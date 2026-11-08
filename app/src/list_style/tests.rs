use super::*;

#[test]
fn a_short_folder_lists_every_page() {
    assert_eq!(listed(0, 3), vec![0, 1, 2]);
    assert_eq!(listed(0, 1), vec![0]);
    assert!(listed(0, 0).is_empty());
    assert_eq!(listed(5, LISTED).len(), usize::try_from(LISTED).unwrap());
}

#[test]
fn a_long_folder_lists_the_pages_around_the_one_on_screen_with_both_ends() {
    let pages = listed(500, 1000);
    assert_eq!(pages.first(), Some(&0));
    assert_eq!(pages.last(), Some(&999));
    assert!(pages.contains(&500) && pages.contains(&401) && pages.contains(&599));
    assert_eq!(pages.len(), usize::try_from(LISTED).unwrap() + 2);
    assert!(pages.windows(2).all(|pair| pair[0] < pair[1]), "in order, each once");
}

#[test]
fn near_an_end_the_list_does_not_run_past_it() {
    let start = listed(3, 1000);
    assert_eq!(start.first(), Some(&0));
    assert_eq!(start.get(1), Some(&1), "no first page added twice");
    assert_eq!(start.last(), Some(&999));
    let end = listed(998, 1000);
    assert_eq!(end.first(), Some(&0));
    assert_eq!(end.last(), Some(&999));
    assert!(end.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(end.iter().all(|page| *page < 1000));
}

#[test]
fn pages_until_the_person_chooses() {
    assert!(preferences::Preferences::default().list_pages.unwrap_or(true));
}
