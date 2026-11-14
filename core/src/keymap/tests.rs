use super::*;

#[test]
fn single_keys_mean_their_action() {
    let mut reader = KeyReader::default();
    assert_eq!(reader.read("j"), Some(Action::Next));
    assert_eq!(reader.read("ArrowUp"), Some(Action::Previous));
    assert_eq!(reader.read("#"), Some(Action::Trash));
    assert_eq!(reader.read("Shift+Space"), Some(Action::ScrollUp));
    assert_eq!(reader.read("q"), None);
}

#[test]
fn a_chord_needs_both_keys_in_time() {
    let mut reader = KeyReader::default();
    let start = Instant::now();
    assert_eq!(reader.read_at("g", start), None);
    assert_eq!(reader.read_at("i", start + Duration::from_millis(500)), Some(Action::Go("inbox")));
    assert_eq!(reader.read_at("g", start), None);
    assert_eq!(reader.read_at("i", start + CHORD + Duration::from_millis(1)), Some(Action::Images));
}

#[test]
fn a_wrong_second_key_only_ends_the_chord() {
    let mut reader = KeyReader::default();
    assert_eq!(reader.read("g"), None);
    assert_eq!(reader.read("q"), None);
    assert_eq!(reader.read("j"), Some(Action::Next));
}

#[test]
fn a_cyrillic_layout_reaches_the_same_keys() {
    assert_eq!(latin_key("\u{441}"), "c");
    assert_eq!(latin_key("\u{43e}"), "j");
    assert_eq!(latin_key("\u{43b}"), "k");
    assert_eq!(latin_key("\u{421}"), "c");
    assert_eq!(latin_key("."), "/");
    let mut reader = KeyReader::default();
    assert_eq!(reader.read("\u{441}"), Some(Action::Compose));
    assert_eq!(reader.read("\u{43f}"), None);
    assert_eq!(reader.read("\u{448}"), Some(Action::Go("inbox")));
}

#[test]
fn other_keys_pass_as_they_are() {
    assert_eq!(latin_key("ArrowDown"), "ArrowDown");
    assert_eq!(latin_key("#"), "#");
    assert_eq!(latin_key(""), "");
}

#[test]
fn the_help_has_one_row_for_each_action() {
    let rows = shortcuts();
    let next = rows.iter().find(|row| row.action == Action::Next);
    assert_eq!(next.map(|row| row.keys.as_str()), Some("j  ↓"));
    assert!(rows.iter().any(|row| row.chord && row.keys == "g i"));
    assert!(rows.iter().all(|row| row.chord == row.keys.starts_with("g ")));
}

#[test]
fn the_two_alphabets_line_up() {
    assert_eq!(CYRILLIC.chars().count(), LATIN.chars().count());
}
