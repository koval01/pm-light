//! Colours for the script editor. Runs joined back together are exactly the input.

use crate::pattern::Pattern;
use serde::Serialize;

const KEYWORDS: &[&str] = &[
    "require",
    "if",
    "elsif",
    "else",
    "stop",
    "keep",
    "discard",
    "fileinto",
    "redirect",
    "reject",
    "vacation",
    "return",
    "set",
    "include",
    "addflag",
    "setflag",
    "removeflag",
    "expire",
    "allof",
    "anyof",
    "not",
    "true",
    "false",
    "exists",
    "header",
    "address",
    "envelope",
    "body",
    "size",
    "environment",
    "spamtest",
    "date",
    "currentdate",
    "hasexpiration",
];

pub(crate) static TOKEN: Pattern = Pattern::new(
    r#"(?P<comment>#[^\n]*|/\*(?s:.*?)(?:\*/|\z))|(?P<string>"(?:[^"\\]|\\(?s:.))*(?:"|\z))|(?P<multiline>text:(?s:.*?)(?:\n\.\r?\n|\z))|(?P<tag>:[A-Za-z_][\w-]*)|(?P<number>\b\d+[KMG]?\b)|(?P<word>[A-Za-z_][\w.]*)"#,
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenKind {
    Comment,
    String,
    Keyword,
    Tag,
    Number,
    Plain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Token {
    pub kind: TokenKind,
    pub text: String,
}

fn kind_of(found: &regex::Captures<'_>) -> TokenKind {
    let has = |name: &str| found.name(name).is_some();
    if has("comment") {
        TokenKind::Comment
    } else if has("string") || has("multiline") {
        TokenKind::String
    } else if has("tag") {
        TokenKind::Tag
    } else if has("number") {
        TokenKind::Number
    } else if found.name("word").is_some_and(|word| KEYWORDS.contains(&word.as_str().to_lowercase().as_str())) {
        TokenKind::Keyword
    } else {
        TokenKind::Plain
    }
}

pub fn tokens(source: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest_from = 0;
    let mut push = |kind, text: Option<&str>| {
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            out.push(Token { kind, text: text.to_string() });
        }
    };
    for found in TOKEN.captures_iter(source) {
        let Some(whole) = found.get(0) else { continue };
        push(TokenKind::Plain, source.get(rest_from..whole.start()));
        push(kind_of(&found), Some(whole.as_str()));
        rest_from = whole.end();
    }
    push(TokenKind::Plain, source.get(rest_from..));
    out
}

#[cfg(test)]
mod tests;
