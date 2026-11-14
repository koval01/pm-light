//! Mail is hostile input, and it is shown in the app's own document.
//!
//! It passes two independent layers: this allowlist, and the app's content security policy, which runs no inline
//! script and reaches no network. Neither relies on the other: what leaves here has no remote address left to
//! fetch, so opening a mail tells its sender nothing even if the policy were missing.

use crate::model::{CleanedLink, MailProtection, ProtectedImage};
use crate::pattern::{Pattern, group};
use crate::text::{plaintext_to_html, to_text};
use ammonia::{Builder, UrlRelative};
use cssparser::{ParseError, Parser, ParserInput, Token};
use lol_html::{
    RewriteStrSettings, element,
    html_content::{ContentType, Element},
    rewrite_str, text,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    sync::LazyLock,
};
use url::Url;

const ALLOWED_TAGS: &[&str] = &[
    "a",
    "abbr",
    "acronym",
    "address",
    "article",
    "aside",
    "b",
    "bdi",
    "bdo",
    "big",
    "blockquote",
    "br",
    "caption",
    "center",
    "cite",
    "code",
    "col",
    "colgroup",
    "dd",
    "del",
    "details",
    "dfn",
    "dir",
    "div",
    "dl",
    "dt",
    "em",
    "figcaption",
    "figure",
    "font",
    "footer",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "i",
    "img",
    "ins",
    "kbd",
    "label",
    "legend",
    "li",
    "main",
    "mark",
    "menu",
    "nav",
    "ol",
    "p",
    "picture",
    "pre",
    "q",
    "rp",
    "rt",
    "ruby",
    "s",
    "samp",
    "section",
    "small",
    "span",
    "strike",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "time",
    "tr",
    "tt",
    "u",
    "ul",
    "var",
    "wbr",
];
const DROPPED_WITH_CONTENT: &[&str] = &[
    "applet",
    "area",
    "audio",
    "embed",
    "frame",
    "frameset",
    "iframe",
    "map",
    "math",
    "noembed",
    "noframes",
    "noscript",
    "object",
    "plaintext",
    "script",
    "select",
    "svg",
    "template",
    "textarea",
    "title",
    "video",
    "xmp",
];
const GENERIC_ATTRIBUTES: &[&str] = &[
    "align",
    "alt",
    "bgcolor",
    "border",
    "cellpadding",
    "cellspacing",
    "class",
    "color",
    "colspan",
    "dir",
    "face",
    "height",
    "hspace",
    "id",
    "lang",
    "name",
    "rowspan",
    "size",
    "span",
    "start",
    "style",
    "title",
    "type",
    "valign",
    "vspace",
    "width",
];
const URL_SCHEMES: &[&str] = &["http", "https", "mailto", "tel", "cid", "data"];
/// Worth telling the reader about; `meta` and the like are in nearly every newsletter.
const REPORTED: &str = "applet, button, embed, form, frame, frameset, iframe, input, object, script, \
                        select, textarea";

/// Past these a mail is an attack or a mistake, and is shown as plain text instead.
const MAX_HTML_BYTES: usize = 4_000_000;
const MAX_TAGS: usize = 60_000;
const MAX_CSS_DEPTH: usize = 32;
/// Browsers stop nesting at 512 themselves.
const MAX_DEPTH: usize = 512;
const SELF_CLOSING: &[&str] = &[
    "area", "base", "br", "col", "dd", "dt", "embed", "hr", "img", "input", "li", "link", "meta", "option", "p",
    "param", "source", "td", "th", "tr", "track", "wbr",
];

/// Marks the app puts on its own blocks in a message being written.
const OWN_CLASSES: &[&str] = &["protonmail_quote", "protonmail_signature_block"];

/// Where a remote image's address waits, in place of `src`, until the reader asks for it.
const HELD_IMAGE: &str = "data-held-src";

const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "gclsrc",
    "dclid",
    "msclkid",
    "twclid",
    "ttclid",
    "yclid",
    "igshid",
    "mc_cid",
    "mc_eid",
    "_hsenc",
    "_hsmi",
    "mkt_tok",
    "vero_id",
    "oly_enc_id",
    "oly_anon_id",
    "ml_subscriber",
    "ml_subscriber_hash",
    "s_cid",
];

static CLEANER: LazyLock<Builder<'static>> = LazyLock::new(|| {
    let mut builder = Builder::default();
    builder
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .clean_content_tags(DROPPED_WITH_CONTENT.iter().copied().collect())
        .generic_attributes(GENERIC_ATTRIBUTES.iter().copied().collect())
        .tag_attributes(HashMap::from([("a", HashSet::from(["href"])), ("img", HashSet::from(["src"]))]))
        .url_schemes(URL_SCHEMES.iter().copied().collect())
        .url_relative(UrlRelative::PassThrough)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        // data: is for inline images alone: as a link it could open a page made by the sender.
        .attribute_filter(|element, attribute, value| {
            let is_data = value.trim_start().get(..5).is_some_and(|s| s.eq_ignore_ascii_case("data:"));
            (!is_data || (element == "img" && attribute == "src")).then_some(value.into())
        });
    builder
});

pub(crate) static PIXEL_STYLE: Pattern =
    Pattern::new(r"(?i)(?:^|;)\s*(?:width|height)\s*:\s*[01]px|display\s*:\s*none");
pub(crate) static TAG: Pattern = Pattern::new(r"<(/?)([a-zA-Z][a-zA-Z0-9]*)");

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

fn walk_css(parser: &mut Parser<'_, '_>, out: &mut String, depth: usize) {
    let mut property = String::new();
    let mut after_colon = false;
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
            Token::Function(name) if name.eq_ignore_ascii_case("url") => {
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

fn is_tracking_param(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("utm_") || TRACKING_PARAMS.contains(&lower.as_str())
}

/// The same address without the parameters that only tell the sender who clicked.
#[must_use]
pub fn clean_link(href: &str) -> String {
    let Ok(mut url) = Url::parse(href.trim()) else {
        return href.to_string();
    };
    if !matches!(url.scheme(), "http" | "https") || !url.query_pairs().any(|(name, _)| is_tracking_param(&name)) {
        return href.to_string();
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(name, _)| !is_tracking_param(name))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect();
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut().clear().extend_pairs(kept);
    }
    url.to_string()
}

enum ImageSource {
    /// Carried by the mail itself.
    Inline,
    Remote(String),
    /// Anything else: it could only reach the app's own files or something stranger.
    Unusable,
}

fn has_prefix(text: &str, prefix: &str) -> bool {
    text.as_bytes().get(..prefix.len()).is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// Reads an address as a browser would: it drops tabs and newlines anywhere in it, and takes a
/// backslash for a slash, so `ht\ntps:` or `https:host` must not pass for something harmless.
fn image_source(src: &str) -> ImageSource {
    let src: String = src.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let src = src.trim_matches(|c: char| c <= ' ').replace('\\', "/");
    if has_prefix(&src, "cid:") || has_prefix(&src, "data:image/") {
        return ImageSource::Inline;
    }
    let absolute = if src.starts_with("//") { format!("https:{src}") } else { src };
    match Url::parse(&absolute) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => ImageSource::Remote(url.to_string()),
        _ => ImageSource::Unusable,
    }
}

fn is_remote(src: &str) -> bool {
    matches!(image_source(src), ImageSource::Remote(_))
}

fn looks_like_pixel(img: &Element<'_, '_>) -> bool {
    let tiny = |name: &str| {
        img.get_attribute(name)
            .is_some_and(|value| value.trim().trim_end_matches("px").parse::<u32>().is_ok_and(|n| n <= 1))
    };
    tiny("width") || tiny("height") || img.get_attribute("style").is_some_and(|s| PIXEL_STYLE.is_match(&s))
}

/// Counts what the cleaner is about to remove, while the elements still exist.
fn active_elements(html: &str) -> Vec<String> {
    let counts = RefCell::new(BTreeMap::<String, usize>::new());
    let _ = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!(REPORTED, |el| {
                *counts.borrow_mut().entry(el.tag_name()).or_default() += 1;
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    );
    counts
        .into_inner()
        .into_iter()
        .map(|(tag, count)| if count > 1 { format!("{tag} × {count}") } else { tag })
        .collect()
}

pub struct Protected {
    pub html: String,
    pub protection: MailProtection,
}

/// How deep tags stack, roughly. Tags that close themselves do not count, so ordinary sloppy HTML
/// stays well below the limit, while the cost of parsing grows quadratically with this number.
fn nesting_depth(html: &str) -> usize {
    let (mut depth, mut deepest) = (0usize, 0usize);
    for tag in TAG.captures_iter(html) {
        if SELF_CLOSING.contains(&group(&tag, 2).to_ascii_lowercase().as_str()) {
            continue;
        }
        if group(&tag, 1).is_empty() {
            depth += 1;
            deepest = deepest.max(depth);
        } else {
            depth = depth.saturating_sub(1);
        }
    }
    deepest
}

/// Sanitizes a mail and lists what was taken out of it or held back.
pub fn protect(html: &str) -> Protected {
    let excessive = html.len() > MAX_HTML_BYTES
        || html.bytes().filter(|b| *b == b'<').count() > MAX_TAGS
        || nesting_depth(html) > MAX_DEPTH;
    if excessive {
        return Protected {
            html: plaintext_to_html(&to_text(html.get(..MAX_HTML_BYTES.min(html.len())).unwrap_or_default())),
            protection: MailProtection { removed: vec!["oversized".to_string()], threats: 1, ..Default::default() },
        };
    }
    let removed = active_elements(html);
    let clean = CLEANER.clean(html).to_string();
    let links = RefCell::new(Vec::new());
    let images = RefCell::new(Vec::<ProtectedImage>::new());
    let css = RefCell::new(String::new());

    let rewritten = rewrite_str(
        &clean,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("a[href]", |el| {
                    el.set_attribute("target", "_blank")?;
                    let from = el.get_attribute("href").unwrap_or_default();
                    let to = clean_link(&from);
                    if to != from {
                        el.set_attribute("href", &to)?;
                        links.borrow_mut().push(CleanedLink { from, to });
                    }
                    Ok(())
                }),
                element!("[style]", |el| {
                    let style = el.get_attribute("style").unwrap_or_default();
                    el.set_attribute("style", &clean_css(&style))?;
                    Ok(())
                }),
                element!("img[src]", |el| {
                    let src = el.get_attribute("src").unwrap_or_default();
                    match image_source(&src) {
                        ImageSource::Inline => {}
                        ImageSource::Unusable => el.remove_attribute("src"),
                        ImageSource::Remote(url) => {
                            let pixel = looks_like_pixel(el);
                            el.remove_attribute("src");
                            el.set_attribute(HELD_IMAGE, &url)?;
                            let mut images = images.borrow_mut();
                            if !images.iter().any(|image| image.url == url) {
                                images.push(ProtectedImage { url, pixel, tracker: String::new(), threat: false });
                            }
                        }
                    }
                    Ok(())
                }),
                text!("style", |chunk| {
                    css.borrow_mut().push_str(chunk.as_str());
                    if chunk.last_in_text_node() {
                        // Raw text to a browser, so a CSS escape keeps it from ending the block.
                        let cleaned = clean_css(&css.borrow()).replace('<', "\\3c ");
                        css.borrow_mut().clear();
                        chunk.replace(&cleaned, ContentType::Html);
                    } else {
                        chunk.remove();
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::new()
        },
    );

    Protected {
        // A rewriter failure must not fall back to anything less clean than the cleaner's output.
        html: rewritten.unwrap_or(clean),
        protection: {
            let mut protection =
                MailProtection { images: images.into_inner(), links: links.into_inner(), removed, threats: 0 };
            protection.settle();
            protection
        },
    }
}

/// The composer edits in the app's own document, outside any shadow root. There a mail's `<style>`
/// would restyle the whole app, and its ids and classes would reach the app's rules and code.
#[must_use]
pub fn for_editor(html: &str) -> String {
    let result = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("style", |el| {
                    el.remove();
                    Ok(())
                }),
                element!("*", |el| {
                    el.remove_attribute("id");
                    el.remove_attribute("name");
                    if !el.get_attribute("class").is_some_and(|name| OWN_CLASSES.contains(&name.as_str())) {
                        el.remove_attribute("class");
                    }
                    Ok(())
                }),
            ],
            ..RewriteStrSettings::new()
        },
    );
    result.unwrap_or_else(|_| plaintext_to_html(&to_text(html)))
}

/// A signature comes from the account, not a stranger, but it enters the same document. Its images
/// stay as they are: they must reach the recipient, and the app's policy keeps them from loading here.
pub fn clean_signature(html: &str) -> String {
    for_editor(&CLEANER.clean(html).to_string())
}

/// Puts fetched images in place. An image that could not be fetched stays blocked.
#[must_use]
pub fn with_images<S: std::hash::BuildHasher>(html: &str, images: &HashMap<String, String, S>) -> String {
    let result = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("img", |el| {
                if let Some(data) = el.get_attribute(HELD_IMAGE).and_then(|url| images.get(&url)) {
                    el.set_attribute("src", data)?;
                    el.remove_attribute(HELD_IMAGE);
                }
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    );
    result.unwrap_or_else(|_| html.to_string())
}

/// The mail with its embedded pictures off the page, or nothing if it has none.
///
/// Remote ones are held by `protect` already. These are here on the machine, so what is kept back is only their
/// showing, and the original stays as it was for when they are asked for.
#[must_use]
pub fn without_embedded(html: &str) -> Option<String> {
    let found = Cell::new(false);
    let result = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![element!("img[src]", |el| {
                if el.get_attribute("src").is_some_and(|src| matches!(image_source(&src), ImageSource::Inline)) {
                    el.remove_attribute("src");
                    found.set(true);
                }
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    );
    result.ok().filter(|_| found.get())
}

#[cfg(test)]
mod parsed_tests;
#[cfg(test)]
mod tests;
