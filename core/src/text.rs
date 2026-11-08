//! Between HTML and plain text: for signatures and auto-replies, which people edit as plain text.

use crate::pattern::{Pattern, group};

const SKIP: &[&str] = &["script", "style", "title"];
const BREAK_AFTER: &[&str] = &[
    "p", "div", "br", "tr", "li", "h1", "h2", "h3", "h4", "h5", "h6", "table", "ul", "ol", "blockquote",
    "section", "header", "footer", "article",
];
const ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", " "),
    ("copy", "©"),
    ("reg", "®"),
    ("hellip", "…"),
    ("mdash", "—"),
    ("ndash", "–"),
    ("laquo", "«"),
    ("raquo", "»"),
];

pub(crate) static TOKEN: Pattern = Pattern::new(r"(?s)<!--.*?-->|<(/?)([a-zA-Z][a-zA-Z0-9]*)\b[^>]*>|[^<]+|<");
pub(crate) static ENTITY: Pattern = Pattern::new(r"(?i)&(#x[0-9a-f]+|#\d+|[a-z]+);");
pub(crate) static SPACES: Pattern = Pattern::new(r"[ \t\r\x0c\x0b]+");

fn decode_entity(entity: &str) -> Option<String> {
    let Some(number) = entity.strip_prefix('#') else {
        let lower = entity.to_lowercase();
        return ENTITIES.iter().find(|(name, _)| *name == lower).map(|(_, value)| (*value).to_string());
    };
    let code = match number.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => number.parse().ok()?,
    };
    (code > 0).then(|| char::from_u32(code)).flatten().map(String::from)
}

fn decode_entities(text: &str) -> String {
    ENTITY
        .replace_all(text, |captures: &regex::Captures<'_>| {
            decode_entity(group(captures, 1)).unwrap_or_else(|| group(captures, 0).to_string())
        })
        .into_owned()
}

/// At most one blank line in a row, and none at either end.
#[must_use]
pub fn collapse(text: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if !line.is_empty() || lines.last().is_some_and(|last| !last.is_empty()) {
            lines.push(line);
        }
    }
    lines.join("\n").trim().to_string()
}

/// Flattens HTML to plain text, keeping paragraph breaks and list items.
pub fn to_text(html: &str) -> String {
    let mut out = String::new();
    let mut skipping = 0usize;
    for token in TOKEN.captures_iter(html) {
        let whole = group(&token, 0);
        if let Some(tag) = token.get(2) {
            let tag = tag.as_str().to_lowercase();
            let closing = !group(&token, 1).is_empty();
            match (closing, tag.as_str()) {
                (false, name) if SKIP.contains(&name) => skipping += 1,
                (false, "br") => out.push('\n'),
                (false, "li") => out.push_str("\n- "),
                (true, name) if SKIP.contains(&name) => skipping = skipping.saturating_sub(1),
                (true, name) if BREAK_AFTER.contains(&name) => out.push('\n'),
                _ => {}
            }
        } else if !whole.starts_with("<!--") && skipping == 0 && !whole.trim().is_empty() {
            out.push_str(&SPACES.replace_all(&decode_entities(whole), " "));
        }
    }
    collapse(&out)
}

#[must_use]
pub fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// A blank line starts a paragraph, a single newline becomes a `<br>`.
#[must_use]
pub fn plaintext_to_html(text: &str) -> String {
    let blocks: Vec<String> = escape(text)
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .map(|block| format!("<p>{}</p>", block.replace('\n', "<br>\n")))
        .collect();
    if blocks.is_empty() { "<p></p>".to_string() } else { blocks.join("\n") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_script_and_style() {
        let html = "<html><head><style>.a{color:red}</style></head><body><script>alert(1)</script><p>Only this.</p></body></html>";
        assert_eq!(to_text(html), "Only this.");
    }

    #[test]
    fn keeps_paragraph_and_line_breaks() {
        assert_eq!(to_text("<p>First.</p><p>Second.</p>"), "First.\nSecond.");
        assert_eq!(to_text("a<br>b"), "a\nb");
    }

    #[test]
    fn list_items_get_bullets() {
        assert_eq!(to_text("<ul><li>one<li>two</ul>"), "- one\n- two");
    }

    #[test]
    fn unescapes_entities() {
        assert_eq!(to_text("<p>3 &lt; 5 &amp; caf&#233; &#x1F600;</p>"), "3 < 5 & café 😀");
    }

    #[test]
    fn leaves_an_unknown_entity_alone() {
        assert_eq!(to_text("a &nosuch; b &#0; c"), "a &nosuch; b &#0; c");
    }

    #[test]
    fn ignores_comments() {
        assert_eq!(to_text("a<!-- <p>no</p> -->b"), "ab");
    }

    #[test]
    fn a_void_tag_does_not_open_a_skip() {
        assert_eq!(to_text("<meta charset=x><p>ok</p>"), "ok");
    }

    #[test]
    fn malformed_html_still_yields_text() {
        let out = to_text("<div><p>text <b>bold</div></p></b>");
        assert!(out.contains("text") && out.contains("bold") && !out.contains('<'));
    }

    #[test]
    fn keeps_a_stray_less_than() {
        assert!(to_text("<p>1 < 2</p>").contains("1 < 2"));
    }

    #[test]
    fn shrinks_a_marketing_mail() {
        let html = "<table><tr><td>&nbsp;</td></tr></table>".repeat(200) + "<p>Your statement is ready.</p>";
        let text = to_text(&html);
        assert!(text.contains("Your statement is ready."));
        assert!(text.len() < html.len() / 10);
    }

    #[test]
    fn collapse_leaves_at_most_one_blank_line() {
        assert_eq!(collapse("a\n\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn plain_text_cannot_inject_markup() {
        let out = plaintext_to_html("a < b & c > d <script>x</script>");
        assert!(out.contains("&lt;") && out.contains("&amp;") && out.contains("&gt;"));
        assert!(!out.contains("<script>"));
    }

    #[test]
    fn newlines_become_breaks_and_paragraphs() {
        assert!(plaintext_to_html("one\ntwo").contains("one<br>"));
        assert_eq!(plaintext_to_html("a\n\nb").matches("<p>").count(), 2);
    }

    #[test]
    fn plain_text_round_trips() {
        let back = to_text(&plaintext_to_html("Hello.\nThis is a < test > with & symbols.\n\nSecond paragraph."));
        assert!(back.contains("a < test > with & symbols.") && back.contains("Second paragraph."));
    }

    #[test]
    fn an_empty_body_is_still_valid_html() {
        assert_eq!(plaintext_to_html(""), "<p></p>");
    }
}
