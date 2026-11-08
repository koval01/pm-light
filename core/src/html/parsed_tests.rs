//! Cleaned mail parsed by the HTML standard's own algorithm, as an engine parses it, then written out
//! and parsed once more, because mutation attacks live in that second reading. Whatever the corpus
//! tried, the document must end up holding only what the allowlist permits. Hostile input: these run
//! through `scripts/attacks.sh`, in a container, never on the host.

use super::{for_editor, protect};
use cssparser::{ParseError, Parser, ParserInput, Token};
use scraper::{Html, Node};

const FRAGMENTS: &str = include_str!("hostile_fragments.json");
const ALLOWED: &str = "a abbr acronym address article aside b bdi bdo big blockquote br caption center cite code col \
    colgroup details dfn dir div dl dt em figcaption figure font footer h1 h2 h3 h4 h5 h6 header hgroup hr i img ins kbd \
    legend li main mark menu nav ol p picture pre q rp rt ruby s samp section small span strike strong style sub \
    summary sup table tbody td tfoot th thead time tr tt u ul var wbr html head body";
const FORBIDDEN_ATTRIBUTES: [&str; 7] = ["srcset", "background", "poster", "action", "formaction", "srcdoc", "data"];
const MIXED_CASES: u32 = 6000;
/// The same corpus each run, so that a failure can be repeated.
const SEED: u32 = 20_261_101;

fn cases() -> Vec<String> {
    let fragments: Vec<String> = serde_json::from_str(FRAGMENTS).unwrap_or_default();
    assert!(fragments.len() > 40, "the corpus did not load");
    let mut state = SEED;
    let mut next = |limit: usize| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        state as usize % limit
    };
    let mut cases = fragments.clone();
    for _ in 0..MIXED_CASES {
        let parts = 2 + next(5);
        cases.push((0..parts).map(|_| fragments[next(fragments.len())].as_str()).collect());
    }
    cases
}

/// An address CSS would fetch: anything but a picture carried in the mail, or an anchor.
fn is_outside(address: &str) -> bool {
    let address = address.trim().to_ascii_lowercase();
    !(address.is_empty() || address.starts_with('#') || address.starts_with("data:image/"))
}

/// Strings inside these are addresses, as `url(` is.
fn takes_addresses(function: &str) -> bool {
    let function = function.to_ascii_lowercase();
    let plain = function.trim_start_matches("-webkit-").trim_start_matches("-moz-");
    ["url", "src", "image", "image-set", "cross-fade"].contains(&plain)
}

/// `position: fixed` or `sticky`, read as tokens, so that comments and escapes change nothing.
fn fixes_in_place(css: &str) -> bool {
    let mut input = ParserInput::new(css);
    let mut parser = Parser::new(&mut input);
    let mut property = String::new();
    let mut after_colon = false;
    while let Ok(token) = parser.next().cloned() {
        match token {
            Token::Ident(name) if after_colon => {
                let name = name.to_ascii_lowercase();
                if property == "position" && (name == "fixed" || name == "sticky") {
                    return true;
                }
                after_colon = false;
            }
            Token::Ident(name) => property = name.to_ascii_lowercase(),
            Token::Colon => after_colon = true,
            _ => after_colon = false,
        }
    }
    false
}

/// Read by a CSS tokenizer, as an engine reads it: escapes such as `u\72l(` are decoded, and text
/// inside a quoted string stays text unless a function takes it for an address.
fn css_problems(parser: &mut Parser<'_, '_>, addresses: bool, found: &mut Vec<String>) {
    while let Ok(token) = parser.next_including_whitespace_and_comments().cloned() {
        match token {
            Token::UnquotedUrl(address) if is_outside(&address) => found.push(format!("css url {address}")),
            Token::BadUrl(address) => found.push(format!("css bad url {address}")),
            Token::QuotedString(address) if addresses && is_outside(&address) && looks_like_address(&address) => {
                found.push(format!("css url {address}"));
            }
            Token::AtKeyword(name) if ["import", "font-face"].contains(&name.to_ascii_lowercase().as_str()) => {
                found.push(format!("css @{name}"));
            }
            Token::Function(name) => {
                let inner = addresses || takes_addresses(&name);
                let _ = parser.parse_nested_block(|block| {
                    css_problems(block, inner, found);
                    Ok::<(), ParseError<'_, ()>>(())
                });
            }
            Token::ParenthesisBlock | Token::SquareBracketBlock | Token::CurlyBracketBlock => {
                let _ = parser.parse_nested_block(|block| {
                    css_problems(block, addresses, found);
                    Ok::<(), ParseError<'_, ()>>(())
                });
            }
            _ => {}
        }
    }
}

/// What an engine resolves to another place: a scheme, or a path from the root or another host.
fn looks_like_address(text: &str) -> bool {
    let text = text.trim();
    text.contains(':') || text.starts_with('/') || text.starts_with('\\')
}

fn css_in(css: &str) -> Vec<String> {
    let mut input = ParserInput::new(css);
    let mut found = Vec::new();
    css_problems(&mut Parser::new(&mut input), false, &mut found);
    found
}

fn is_scheme_allowed(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    ["http:", "https:", "mailto:", "tel:", "cid:", "#"].iter().any(|start| value.starts_with(start))
        || !value.contains(':')
}

fn problems_in(document: &Html, styles: bool) -> Vec<String> {
    let allowed: Vec<&str> = ALLOWED.split_whitespace().collect();
    let mut found = Vec::new();
    for node in document.tree.nodes() {
        let Node::Element(element) = node.value() else { continue };
        let tag = element.name();
        if !allowed.contains(&tag) {
            found.push(format!("tag {tag}"));
        }
        if tag == "style" {
            let text: String = node.children().filter_map(|child| child.value().as_text()).map(|text| text.to_string()).collect();
            if !styles {
                found.push("style in the editor".to_string());
            }
            if text.contains('<') {
                found.push("angle bracket in style".to_string());
            }
            found.extend(css_in(&text));
        }
        for (name, value) in element.attrs() {
            if name.starts_with("on") {
                found.push(format!("handler {name}"));
            }
            if name == "href" && !is_scheme_allowed(value) {
                found.push(format!("href {value}"));
            }
            let source = value.trim().to_ascii_lowercase();
            if name == "src" && !source.starts_with("cid:") && !source.starts_with("data:image/") {
                found.push(format!("src {value}"));
            }
            if FORBIDDEN_ATTRIBUTES.contains(&name) {
                found.push(format!("attribute {name}"));
            }
            if name == "style" {
                found.extend(css_in(value));
                if fixes_in_place(value) {
                    found.push("fixed position".to_string());
                }
            }
        }
    }
    found
}

/// Both readings: as the mail arrives in the document, and after being written out again.
fn problems(html: &str, styles: bool) -> Vec<String> {
    let wrapped = format!(r#"<div class="content">{html}</div>"#);
    let first = Html::parse_fragment(&wrapped);
    let again = Html::parse_fragment(&first.html());
    let mut found = problems_in(&first, styles);
    found.extend(problems_in(&again, styles));
    found
}

#[test]
#[ignore = "hostile input: run by scripts/attacks.sh in a container"]
fn no_mail_gets_past_the_cleaning_in_a_parsed_document() {
    let cases = cases();
    let mut failed = Vec::new();
    for input in &cases {
        let mail = protect(input).html;
        let editor = for_editor(&mail);
        for (field, html, styles) in [("mail", &mail, true), ("editor", &editor, false)] {
            failed.extend(problems(html, styles).into_iter().map(|why| format!("{field}: {why}: {input}")));
        }
    }
    println!("{} mails parsed twice, {} problems", cases.len(), failed.len());
    failed.iter().take(6).for_each(|problem| println!("{problem}"));
    assert!(failed.is_empty());
}

#[test]
fn the_check_itself_catches_what_it_is_for() {
    for (html, why) in [
        ("<img src=https://evil.example/a>", "src"),
        ("<a href=javascript:alert(1)>x</a>", "href"),
        ("<div onclick=x>y</div>", "handler"),
        ("<script>x</script>", "tag script"),
        (r#"<div style="background:u\72l(https://evil.example/a)">x</div>"#, "css url"),
        ("<style>@import 'x';</style>", "css @import"),
        ("<style>@font-face{src:local(x)}</style>", "css @font-face"),
        (r#"<style>a{background:image-set("https://evil.example/a" 1x)}</style>"#, "css url"),
        (r#"<div style="background:url('//evil.example/a')">x</div>"#, "css url"),
        (r#"<div style="position:fixed">x</div>"#, "fixed"),
        (r#"<div style="position:/**/f\69xed">x</div>"#, "fixed"),
        ("<table background=x>", "attribute background"),
    ] {
        assert!(problems(html, true).iter().any(|found| found.starts_with(why)), "{html} must be caught as {why}");
    }
}

#[test]
fn the_check_leaves_clean_mail_alone() {
    for html in [
        r#"<p style="color:red">a <a href="https://proton.me">b</a><img src="data:image/png;base64,AA"></p>"#,
        // Words in quotes are words: an engine fetches nothing for them.
        r#"<style>p::after{content:"url(https://proton.me)"}</style>"#,
        r#"<div style="background:url(data:image/png;base64,AA);position:static">x</div>"#,
    ] {
        assert_eq!(problems(html, true), Vec::<String>::new(), "{html}");
    }
}
