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
