use super::*;
use slint::Model;

#[test]
fn a_code_keeps_digits_only() {
    assert_eq!(code_digits("12 3a4"), "1234");
}

#[test]
fn a_pasted_code_stops_at_six() {
    assert_eq!(code_digits("12345678"), "123456");
    assert!(is_whole_code(&code_digits("12345678")));
}

#[test]
fn a_short_code_is_not_whole() {
    assert!(!is_whole_code("12345"));
}

fn pieces(lines: &ModelRc<CodeLine>) -> Vec<Vec<(String, i32)>> {
    lines.iter().map(|line| line.pieces.iter().map(|piece| (piece.text.to_string(), piece.kind)).collect()).collect()
}

#[test]
fn a_script_keeps_its_lines_and_its_colours() {
    let found = pieces(&code_lines("# note\n\nif true {"));
    assert_eq!(found.len(), 3);
    assert_eq!(found[0], [("#\u{a0}note".to_string(), 1)]);
    assert_eq!(found[1], [("\u{a0}".to_string(), 0)]);
    assert!(found[2].iter().any(|(text, kind)| text == "if" && *kind == 3));
}

#[test]
fn a_comment_over_several_lines_stays_a_comment() {
    let found = pieces(&code_lines("/* one\ntwo */"));
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|line| line.iter().all(|(_, kind)| *kind == 1)));
}

#[test]
fn money_is_written_as_the_language_writes_it() {
    assert_eq!(money_in("en", 1_234_567, "USD"), "$12,345.67");
    assert_eq!(money_in("de", 1_234_567, "EUR"), "12.345,67\u{a0}€");
    assert_eq!(money_in("ru", 1_234_567, "EUR"), "12\u{a0}345,67\u{a0}€");
    assert_eq!(money_in("en", 500, "CHF"), "CHF\u{a0}5.00");
    assert_eq!(money_in("en", -205, "EUR"), "-€2.05");
    assert_eq!(money_in("de", 0, "EUR"), "0,00\u{a0}€");
}

#[test]
fn sizes_use_the_units_given() {
    assert_eq!(size_in("B,KB,MB", 799), "799 B");
    assert_eq!(size_in("o,Ko,Mo", 2048), "2.0 Ko");
    assert_eq!(size_in("B,KB,MB", 64 * 1024), "64 KB");
}
