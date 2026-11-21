//! A mail's CSS, read with a CSS tokenizer and written back without what fetches, runs, or draws over the reader.

use super::is_remote;
use cssparser::{ParseError, Parser, ParserInput, Token};

const MAX_CSS_DEPTH: usize = 32;
/// Shadows a mail may stack on one box or text. Each layer is blurred on its own, and a thousand of them held a CPU
/// engine on one screen for half a minute (docs/BLITZ.md).
const MAX_SHADOW_LAYERS: usize = 4;

fn is_shadow(property: &str) -> bool {
    ["box-shadow", "text-shadow", "-webkit-box-shadow", "-moz-box-shadow"]
        .iter()
        .any(|shadow| property.eq_ignore_ascii_case(shadow))
}

/// Passes over the rest of a declaration, through its `;`. False when the input ended first.
fn skip_declaration(parser: &mut Parser<'_, '_>) -> bool {
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        if matches!(token, Token::Semicolon) {
            return true;
        }
    }
    false
}

/// At-rules that fetch something or reach outside the mail's own styles.
fn is_blocked_at_rule(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "import" | "font-face" | "namespace" | "charset" | "document" | "-moz-document"
    )
}

fn is_blocked_property(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "behavior" | "-moz-binding")
}

/// Only images carried inside the mail itself. Anything else would contact a server.
fn is_safe_css_url(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.is_empty() || url.starts_with('#') || url.starts_with("data:image/")
}

fn skip_at_rule(parser: &mut Parser<'_, '_>) {
    while let Ok(token) = parser.next() {
        match token {
            Token::Semicolon => return,
            Token::CurlyBracketBlock => {
                let _ = parser.parse_nested_block(|block| {
                    while block.next().is_ok() {}
                    Ok::<(), ParseError<'_, ()>>(())
                });
                return;
            }
            _ => {}
        }
    }
}

/// `url(...)` written as a quoted address when it is one of the mail's own, and as `none` otherwise.
fn write_url(parser: &mut Parser<'_, '_>, out: &mut String) {
    let mut inner = String::new();
    let safe = parser
        .parse_nested_block(|p| {
            let value = p.expect_string().map_or_default(std::string::ToString::to_string);
            inner = value;
            Ok::<(), ParseError<'_, ()>>(())
        })
        .is_ok()
        && is_safe_css_url(&inner);
    if safe {
        out.push_str("url(\"");
        out.push_str(&inner.replace(['\\', '"'], ""));
        out.push_str("\")");
    } else {
        out.push_str("none");
    }
}

fn walk_css(parser: &mut Parser<'_, '_>, out: &mut String, depth: usize) {
    let mut property = String::new();
    let mut after_colon = false;
    // The property of the declaration being read, and how many layers of it have been written.
    let (mut declaring, mut last_ident, mut layers) = (String::new(), String::new(), 0usize);
    loop {
        let start = parser.position();
        let Ok(token) = parser.next_including_whitespace_and_comments().cloned() else {
            return;
        };
        let source = parser.slice_from(start).to_string();
        match token {
            // A space, not nothing: `</**//style>` must not close up into a tag.
            Token::Comment(_) => out.push(' '),
            Token::AtKeyword(name) if is_blocked_at_rule(&name) => skip_at_rule(parser),
            Token::UnquotedUrl(url) => out.push_str(if is_safe_css_url(&url) { &source } else { "none" }),
            // The parser gave up on these, so nothing can be said about what they would do.
            Token::BadUrl(_) | Token::BadString(_) => out.push_str("none"),
            Token::QuotedString(value) if depth > 0 && !is_safe_css_url(&value) && is_remote(&value) => {
                out.push_str("\"\"");
            }
            Token::Function(name) if name.eq_ignore_ascii_case("expression") => {
                out.push_str("none");
                let _ = parser.parse_nested_block(|p| {
                    while p.next().is_ok() {}
                    Ok::<(), ParseError<'_, ()>>(())
                });
            }
            Token::Function(name) if name.eq_ignore_ascii_case("url") => write_url(parser, out),
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                out.push_str(&source);
                let closing = match token {
                    Token::SquareBracketBlock => ']',
                    Token::CurlyBracketBlock => '}',
                    _ => ')',
                };
                if depth < MAX_CSS_DEPTH {
                    let _ = parser.parse_nested_block(|p| {
                        walk_css(p, out, depth + 1);
                        Ok::<(), ParseError<'_, ()>>(())
                    });
                }
                out.push(closing);
            }
            Token::Ident(name)
                if after_colon
                    && property.eq_ignore_ascii_case("position")
                    && (name.eq_ignore_ascii_case("fixed") || name.eq_ignore_ascii_case("sticky")) =>
            {
                out.push_str("static");
                after_colon = false;
            }
            Token::Ident(name) => {
                last_ident = name.to_string();
                if is_blocked_property(&name) {
                    out.push_str("x-removed");
                } else {
                    out.push_str(&source);
                }
                if !after_colon {
                    property = name.to_string();
                }
                after_colon = false;
            }
            Token::Colon => {
                after_colon = true;
                declaring.clone_from(&last_ident);
                layers = 1;
                out.push_str(&source);
            }
            Token::Comma if is_shadow(&declaring) => {
                after_colon = false;
                layers += 1;
                if layers <= MAX_SHADOW_LAYERS {
                    out.push_str(&source);
                } else {
                    if skip_declaration(parser) {
                        out.push(';');
                    }
                    declaring.clear();
                }
            }
            Token::Semicolon => {
                after_colon = false;
                declaring.clear();
                out.push_str(&source);
            }
            Token::WhiteSpace(_) => out.push_str(&source),
            _ => {
                after_colon = false;
                out.push_str(&source);
            }
        }
    }
}

/// CSS that fetches, runs, or lets a mail draw over the reader. Parsed rather than searched: the
/// parser decodes `\\75 rl(`, which any text search would miss.
#[must_use]
pub fn clean_css(css: &str) -> String {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut out = String::with_capacity(css.len());
    walk_css(&mut parser, &mut out, 0);
    out
}
