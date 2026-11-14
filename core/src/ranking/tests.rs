use super::*;

fn labels(all: &[&str]) -> Vec<String> {
    all.iter().map(|label| (*label).to_string()).collect()
}

#[test]
fn an_empty_query_keeps_everything_in_order() {
    assert_eq!(rank(&labels(&["b", "a"]), "  "), [0, 1]);
}

#[test]
fn a_start_beats_a_word_start_beats_the_middle() {
    let all = labels(&["reinbox", "go inbox", "inbox"]);
    assert_eq!(rank(&all, "inbox"), [2, 1, 0]);
}

#[test]
fn letters_in_order_still_match_last() {
    assert_eq!(rank(&labels(&["moved", "archive"]), "mvd"), [0]);
    assert_eq!(rank(&labels(&["zebra"]), "bz"), Vec::<usize>::new());
}

#[test]
fn neither_case_nor_accents_matter() {
    assert_eq!(rank(&labels(&["\u{dc}ber uns"]), "uber"), [0]);
    assert_eq!(rank(&labels(&["\u{415}\u{449}\u{451}"]), "\u{415}\u{429}\u{415}"), [0]);
}

#[test]
fn multi_byte_text_cannot_break_the_search() {
    assert_eq!(rank(&labels(&["🦀 inbox", "\u{44f}\u{449}\u{438}\u{43a}"]), "inbox"), [0]);
    assert_eq!(rank(&labels(&["\u{44f}\u{449}\u{438}\u{43a} \u{432}\u{445}"]), "\u{432}\u{445}"), [0]);
}

#[test]
fn containing_wants_the_query_whole() {
    let all = labels(&["Undo send", "Under the hood"]);
    assert_eq!(containing(&all, "undo"), [0]);
    assert_eq!(containing(&all, ""), [0, 1]);
}
