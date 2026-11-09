use super::*;

#[test]
fn the_report_counts_growth_from_the_first_round() {
    let text = report(&[310, 318, 321, 322]);
    assert_eq!(text, "round 1 MB=310\nround 2 MB=318\nround 3 MB=321\nround 4 MB=322\ngrew MB=12\n");
}

#[test]
fn memory_given_back_is_no_growth() {
    assert!(report(&[300, 280]).ends_with("grew MB=0\n"));
}

#[test]
fn no_figures_make_an_empty_report() {
    assert_eq!(report(&[]), "");
}

// One round to fill and several to measure, or growth cannot be told from filling.
const _: () = assert!(ROUNDS >= 4);
