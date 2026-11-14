use super::*;

#[test]
fn a_cyrillic_key_means_the_letter_in_its_place() {
    assert_eq!(translated(Some("\u{43c}"), KeyCode::KeyV), Some("v"));
    assert_eq!(translated(Some("\u{441}"), KeyCode::KeyC), Some("c"));
    assert_eq!(translated(Some("\u{44f}"), KeyCode::KeyZ), Some("z"));
}

#[test]
fn a_latin_layout_is_left_alone() {
    assert_eq!(translated(Some("v"), KeyCode::KeyV), None);
    assert_eq!(translated(Some("k"), KeyCode::KeyV), None);
}

#[test]
fn other_keys_are_left_alone() {
    assert_eq!(translated(Some("\u{439}"), KeyCode::KeyQ), None);
    assert_eq!(translated(Some("1"), KeyCode::Digit1), None);
}
