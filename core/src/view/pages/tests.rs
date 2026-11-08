use super::super::{Filters, StoredList, View, tests::row};
use crate::model::MessageMeta;

fn rows(from: usize, count: usize) -> Vec<MessageMeta> {
    (from..from + count).map(|at| row(&at.to_string())).collect()
}

fn ids(view: &View) -> Vec<String> {
    view.update(true, Vec::new(), Vec::new()).rows.unwrap_or_default().into_iter().map(|row| row.id).collect()
}

/// A paged inbox of `found` rows, showing its first page.
fn paged(found: u64) -> View {
    let mut view = View::default();
    view.set_paged(true);
    let mark = view.open("inbox");
    assert!(view.loaded(mark, rows(0, usize::try_from(found.min(50)).unwrap())));
    view.counted(Some(found));
    view
}

#[test]
fn a_list_that_grows_has_no_pages() {
    let mut view = View::default();
    let mark = view.open("inbox");
    assert!(view.loaded(mark, rows(0, 50)));
    view.counted(Some(500));
    assert_eq!(view.pages(), 0);
    assert_eq!(view.first_row(), 0);
    assert_eq!(view.to_page(3), None);
    assert!(view.begin_more().is_some());
}

#[test]
fn pages_are_counted_from_proton_total() {
    assert_eq!(paged(120).pages(), 3);
    assert_eq!(paged(100).pages(), 2);
    assert_eq!(paged(101).pages(), 3);
    assert_eq!(paged(1).pages(), 1);
}

#[test]
fn an_empty_folder_is_one_empty_page() {
    assert_eq!(paged(0).pages(), 1);
}

#[test]
fn an_uncounted_list_has_no_pages_even_when_paged() {
    let mut view = paged(120);
    view.counted(None);
    assert_eq!(view.pages(), 0);
    assert!(view.begin_more().is_some(), "a search checked here still reads on as it is scrolled");
}

#[test]
fn a_page_starts_where_the_ones_before_it_end() {
    let mut view = paged(120);
    assert!(view.to_page(2).is_some());
    assert_eq!(view.first_row(), 100);
    let update = view.update(false, Vec::new(), Vec::new());
    assert_eq!((update.page_at, update.pages), (2, 3));
}

#[test]
fn a_page_past_the_last_is_the_last() {
    let mut view = paged(120);
    assert!(view.to_page(u64::MAX).is_some());
    assert_eq!(view.first_row(), 100);
}

#[test]
fn the_page_on_screen_is_not_loaded_again() {
    let mut view = paged(120);
    assert_eq!(view.to_page(0), None);
}

#[test]
fn a_page_holds_only_its_own_rows() {
    let mut view = paged(120);
    let mark = view.to_page(1).unwrap();
    assert!(view.loaded(mark, rows(50, 50)));
    let shown = ids(&view);
    assert_eq!(shown.len(), 50);
    assert_eq!(shown.first().map(String::as_str), Some("50"));
    assert!(!shown.contains(&"0".to_string()));
}

#[test]
fn reloading_a_first_page_keeps_nothing_beyond_it() {
    let mut view = paged(120);
    let mark = view.begin();
    assert!(view.loaded(mark, rows(0, 50)));
    assert_eq!(ids(&view).len(), 50);
}

#[test]
fn moving_to_a_page_lets_go_of_the_open_and_ticked_mail() {
    let mut view = paged(120);
    view.select(Some("3".to_string()));
    view.toggle("4");
    assert!(view.to_page(1).is_some());
    assert_eq!(view.selected(), None);
    assert!(view.targets().is_empty());
}

#[test]
fn a_later_page_has_no_new_mail() {
    let mut view = paged(120);
    let mark = view.to_page(1).unwrap();
    let mut newer = rows(900, 50);
    for row in &mut newer {
        row.date = 10;
    }
    assert!(view.loaded(mark, newer));
    assert!(view.update(false, Vec::new(), Vec::new()).fresh.is_empty());
}

#[test]
fn a_paged_list_does_not_grow_by_scrolling() {
    let mut view = paged(120);
    assert_eq!(view.begin_more(), None);
}

#[test]
fn another_folder_or_filter_starts_at_the_first_page() {
    let mut view = paged(120);
    assert!(view.to_page(2).is_some());
    let _ = view.open("archive");
    assert_eq!(view.first_row(), 0);

    let mut view = paged(120);
    assert!(view.to_page(2).is_some());
    let _ = view.set_filters(Filters { unread: true, ..Filters::default() });
    assert_eq!(view.first_row(), 0);
}

#[test]
fn a_new_page_size_starts_at_the_first_page() {
    let mut view = paged(120);
    assert!(view.to_page(2).is_some());
    view.set_page(100);
    assert_eq!(view.first_row(), 0);
}

#[test]
fn switching_between_pages_and_one_list_starts_at_the_top() {
    let mut view = paged(120);
    assert!(view.to_page(2).is_some());
    view.set_paged(false);
    assert_eq!((view.first_row(), view.pages()), (0, 0));
    view.set_paged(true);
    assert_eq!(view.first_row(), 0);
}

#[test]
fn a_later_page_is_not_kept_as_the_folder() {
    let mut view = paged(120);
    let mark = view.to_page(1).unwrap();
    assert!(view.loaded(mark, rows(50, 50)));
    view.confirm(1);
    assert!(view.as_stored().is_none());
    let _ = view.open("archive");
    assert!(!view.has_shelf("inbox"));
}

#[test]
fn a_first_page_kept_from_before_comes_back_with_its_count() {
    let mut view = paged(120);
    view.confirm(1);
    let _ = view.open("archive");
    let _ = view.open("inbox");
    assert_eq!(view.pages(), 3);
}

#[test]
fn a_long_list_kept_from_before_shows_one_page() {
    let mut view = View::default();
    let mark = view.open("inbox");
    let mut long = rows(0, 50);
    long.extend(rows(50, 50));
    assert!(view.loaded(mark, long));
    let stored: StoredList = view.as_stored().unwrap();
    let mut paged = View::default();
    paged.set_paged(true);
    paged.stock("inbox", stored);
    let _ = paged.open("inbox");
    assert_eq!(ids(&paged).len(), 50);
    assert!(paged.update(false, Vec::new(), Vec::new()).has_more);
}

#[test]
fn an_emptied_last_page_steps_back_to_the_last_there_is() {
    let mut view = paged(260);
    let mark = view.to_page(5).unwrap();
    assert!(view.loaded(mark, Vec::new()));
    view.counted(Some(140));
    assert!(view.step_back_if_past());
    assert_eq!(view.first_row(), 100);
}

#[test]
fn an_emptied_page_steps_back_one_when_the_count_did_not_change() {
    let mut view = paged(260);
    let mark = view.to_page(5).unwrap();
    assert!(view.loaded(mark, Vec::new()));
    assert!(view.step_back_if_past());
    assert_eq!(view.first_row(), 200);
}

#[test]
fn a_page_with_mail_or_the_first_page_never_steps_back() {
    let mut view = paged(120);
    let mark = view.begin();
    assert!(view.loaded(mark, Vec::new()));
    assert!(!view.step_back_if_past());

    let mut view = paged(120);
    let mark = view.to_page(2).unwrap();
    assert!(view.loaded(mark, rows(100, 20)));
    assert!(!view.step_back_if_past());
}

#[test]
fn emptying_a_folder_goes_back_to_the_first_page() {
    let mut view = paged(120);
    assert!(view.to_page(2).is_some());
    view.clear();
    assert_eq!(view.first_row(), 0);
}

#[test]
fn a_long_list_turned_into_pages_shows_one_page_after_its_reload() {
    let mut view = View::default();
    let mark = view.open("inbox");
    assert!(view.loaded(mark, rows(0, 50)));
    let more = view.begin_more().unwrap();
    assert!(view.appended(more, rows(50, 50)));
    view.set_paged(true);
    let mark = view.begin();
    assert!(view.loaded(mark, rows(0, 50)));
    assert_eq!(ids(&view).len(), 50);
}
