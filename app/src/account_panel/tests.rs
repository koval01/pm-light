use super::initials;

#[test]
fn the_picture_takes_the_first_letters_of_the_name() {
    assert_eq!(initials("Yaroslav Koval", "y@example.com"), "YK");
    // Letters outside ASCII are made capital too, written as escapes to keep the source in English.
    assert_eq!(initials("\u{e9}lodie \u{142}ukasz", "e@example.com"), "\u{c9}\u{141}");
    assert_eq!(initials("Anna Maria Schmidt", "a@example.com"), "AM");
    assert_eq!(initials("  madonna  ", "m@example.com"), "M");
}

#[test]
fn without_a_name_the_address_gives_the_letter() {
    assert_eq!(initials("", "koval@example.com"), "K");
    assert_eq!(initials("   ", "_x@example.com"), "X");
    assert_eq!(initials("", ""), "");
}

#[test]
fn signs_in_a_name_are_passed_over() {
    assert_eq!(initials("(Work) Koval", ""), "WK");
    assert_eq!(initials("— ·", "q@example.com"), "Q");
}
