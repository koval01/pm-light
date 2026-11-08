//! Colours for the script editor. Runs joined back together are exactly the input.

use crate::pattern::Pattern;
use serde::Serialize;

const KEYWORDS: &[&str] = &[
    "require", "if", "elsif", "else", "stop", "keep", "discard", "fileinto", "redirect", "reject", "vacation", "return",
    "set", "include", "addflag", "setflag", "removeflag", "expire", "allof", "anyof", "not", "true", "false", "exists",
    "header", "address", "envelope", "body", "size", "environment", "spamtest", "date", "currentdate", "hasexpiration",
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
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<(TokenKind, String)> {
        tokens(source).into_iter().filter(|t| !t.text.trim().is_empty()).map(|t| (t.kind, t.text)).collect()
    }

    #[test]
    fn colours_each_kind() {
        let found = kinds(r#"if header :contains "x" { keep; } # done"#);
        assert_eq!(found[0], (TokenKind::Keyword, "if".to_string()));
        assert_eq!(found[1], (TokenKind::Keyword, "header".to_string()));
        assert_eq!(found[2], (TokenKind::Tag, ":contains".to_string()));
        assert_eq!(found[3], (TokenKind::String, "\"x\"".to_string()));
        assert_eq!(found.last().unwrap().0, TokenKind::Comment);
    }

    #[test]
    fn numbers_and_plain_words_stay_apart() {
        assert_eq!(kinds("size :over 10M")[2], (TokenKind::Number, "10M".to_string()));
        assert_eq!(kinds("myword")[0].0, TokenKind::Plain);
    }

    #[test]
    fn unfinished_text_is_coloured_to_its_end() {
        assert_eq!(kinds("\"open").len(), 1);
        assert_eq!(kinds("/* open").len(), 1);
        assert_eq!(kinds("text:\nabc").len(), 1);
    }

    #[test]
    fn nothing_is_lost_whatever_is_typed() {
        for source in ["", "   ", "a\"b\\", "#", "🦀 \"🦀\" /* 🦀", "if\r\n\ttrue {\n}\n", "\\", ":", ":a-b_c 1K"] {
            let joined: String = tokens(source).into_iter().map(|t| t.text).collect();
            assert_eq!(joined, source);
        }
    }
}
