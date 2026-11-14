//! Between HTML and plain text: for signatures and auto-replies, which people edit as plain text.

use crate::pattern::{Pattern, group};

const SKIP: &[&str] = &["script", "style", "title"];
const BREAK_AFTER: &[&str] = &[
    "p",
    "div",
    "br",
    "tr",
    "li",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "table",
    "ul",
    "ol",
    "blockquote",
    "section",
    "header",
    "footer",
    "article",
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
mod tests;
