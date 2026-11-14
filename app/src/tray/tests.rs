use super::*;

#[test]
fn shows_nothing_for_an_empty_inbox() {
    assert_eq!(count_text(0), None);
}

#[test]
fn shows_the_count() {
    assert_eq!(count_text(27).as_deref(), Some("27"));
}

#[test]
fn caps_a_huge_count() {
    assert_eq!(count_text(12_000).as_deref(), Some("999+"));
}
