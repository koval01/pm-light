use super::*;
use crate::model::Address;

pub(super) fn row(id: &str) -> MessageMeta {
    MessageMeta {
        id: id.to_string(),
        folder: "inbox".into(),
        from: Address { name: None, email: "a@b.c".into() },
        to: Vec::new(),
        subject: String::new(),
        snippet: String::new(),
        date: 0,
        unread: true,
        starred: false,
        labels: Vec::new(),
        attachment_count: 0,
        conversation_id: format!("c{id}"),
        messages: 0,
        expires: 0,
    }
}

fn showing(ids: &[&str]) -> View {
    let mut view = View::default();
    let mark = view.open("inbox");
    assert!(view.loaded(mark, ids.iter().map(|id| row(id)).collect()));
    view
}

fn ids(all: &[&str]) -> Vec<String> {
    all.iter().map(|id| (*id).to_string()).collect()
}

fn listed(view: &View) -> Vec<String> {
    view.update(true, Vec::new(), Vec::new()).rows.unwrap().into_iter().map(|row| row.id).collect()
}

#[test]
fn sent_and_drafts_cannot_go_to_inbox_or_spam() {
    assert!(!can_move("sent", "inbox") && !can_move("drafts", "spam"));
    assert!(can_move("sent", "trash") && can_move("inbox", "spam"));
}

#[test]
fn the_page_learns_which_targets_are_refused() {
    let mut view = showing(&[]);
    assert!(view.update(false, vec![], vec![]).refused_targets.is_empty());
    view.open("sent");
    assert_eq!(view.update(false, vec![], vec![]).refused_targets, ["inbox", "spam"]);
}

#[test]
fn an_answer_for_an_old_folder_is_dropped() {
    let mut view = View::default();
    let old = view.open("spam");
    let new = view.open("inbox");
    assert!(!view.loaded(old, vec![row("stale")]));
    assert!(view.loaded(new, vec![row("fresh")]));
    assert_eq!(listed(&view), ["fresh"]);
    assert!(!view.failed(old, Value::Null));
}

#[test]
fn opening_a_folder_clears_the_last_one() {
    let mut view = showing(&["1"]);
    view.select(Some("1".into()));
    view.toggle("1");
    view.set_filters(Filters { unread: true, ..Filters::default() });
    view.open("sent");
    assert_eq!((view.selected(), view.targets().len(), view.filters().unread), (None, 0, false));
    assert!(listed(&view).is_empty());
}

#[test]
fn a_full_page_means_there_may_be_more() {
    let mut view = View::default();
    let mark = view.open("inbox");
    let page: Vec<MessageMeta> = (0..PAGE).map(|i| row(&i.to_string())).collect();
    view.loaded(mark, page);
    assert!(view.begin_more().is_some());
    assert!(view.begin_more().is_none(), "one load at a time");
}

#[test]
fn a_short_page_is_the_last() {
    assert!(showing(&["1"]).begin_more().is_none());
}

#[test]
fn pages_do_not_repeat_rows() {
    let mut view = View::default();
    let mark = view.open("inbox");
    view.loaded(mark, (0..PAGE).map(|i| row(&i.to_string())).collect());
    let mark = view.begin_more().unwrap();
    assert!(view.appended(mark, vec![row("0"), row("new")]));
    assert_eq!(view.offset(), PAGE + 1);
}

#[test]
fn stepping_stays_inside_the_list() {
    let mut view = showing(&["1", "2", "3"]);
    assert_eq!(view.relative(1).as_deref(), Some("1"), "from nothing, the start");
    assert_eq!(view.relative(-1).as_deref(), Some("3"), "from nothing, the end");
    view.select(Some("2".into()));
    assert_eq!(view.relative(1).as_deref(), Some("3"));
    assert_eq!(view.relative(100).as_deref(), Some("3"));
    assert_eq!(view.relative(-100).as_deref(), Some("1"));
    view.select(Some("gone".into()));
    assert_eq!(view.relative(1).as_deref(), Some("1"));
}

#[test]
fn an_empty_list_has_nowhere_to_step() {
    assert_eq!(showing(&[]).relative(1), None);
}

#[test]
fn ticked_messages_win_over_the_open_one() {
    let mut view = showing(&["1", "2"]);
    view.select(Some("1".into()));
    assert_eq!(view.targets(), ids(&["1"]));
    view.toggle("2");
    assert_eq!(view.targets(), ids(&["2"]));
    view.toggle("2");
    assert_eq!(view.targets(), ids(&["1"]));
    view.check_all();
    assert_eq!(view.targets(), ids(&["1", "2"]));
    view.clear_checks();
    assert_eq!(view.targets(), ids(&["1"]));
}

#[test]
fn the_next_message_opens_when_the_open_one_leaves() {
    let mut view = showing(&["1", "2", "3"]);
    view.select(Some("2".into()));
    view.take(&ids(&["2"]), true);
    assert_eq!(view.selected(), Some("3"));
    assert_eq!(listed(&view), ids(&["1", "3"]));
}

#[test]
fn at_the_end_the_previous_one_opens() {
    let mut view = showing(&["1", "2"]);
    view.select(Some("2".into()));
    view.take(&ids(&["2"]), true);
    assert_eq!(view.selected(), Some("1"));
}

#[test]
fn nothing_opens_unless_the_person_wants_that() {
    let mut view = showing(&["1", "2"]);
    view.select(Some("1".into()));
    view.take(&ids(&["1"]), false);
    assert_eq!(view.selected(), None);
}

#[test]
fn moving_several_skips_those_leaving_too() {
    let mut view = showing(&["1", "2", "3", "4"]);
    view.select(Some("1".into()));
    view.take(&ids(&["1", "2", "3"]), true);
    assert_eq!(view.selected(), Some("4"));
}

#[test]
fn another_row_leaving_keeps_the_open_one() {
    let mut view = showing(&["1", "2"]);
    view.select(Some("1".into()));
    view.toggle("2");
    view.take(&ids(&["2"]), true);
    assert_eq!(view.selected(), Some("1"));
    assert!(view.update(false, vec![], vec![]).checked.is_empty());
}

#[test]
fn marks_change_only_the_named_rows_and_say_which() {
    let mut view = showing(&["1", "2"]);
    let changed = view.mark(&ids(&["1"]), &MessageChanges { unread: Some(false), ..Default::default() });
    assert_eq!(changed.len(), 1);
    assert!(!changed[0].unread);
    assert!(view.row("2").unwrap().unread);
}

#[test]
fn labels_do_not_repeat() {
    let mut view = showing(&["1"]);
    view.label(&ids(&["1"]), "x", true);
    view.label(&ids(&["1"]), "x", true);
    assert_eq!(view.row("1").unwrap().labels, ["x"]);
    view.label(&ids(&["1"]), "x", false);
    assert!(view.row("1").unwrap().labels.is_empty());
}

#[test]
fn rows_travel_only_when_asked() {
    let view = showing(&["1"]);
    assert!(view.update(false, vec![], vec![]).rows.is_none());
    assert_eq!(view.update(true, vec![], vec![]).rows.unwrap().len(), 1);
}

#[test]
fn a_folder_keeps_its_rows_for_a_return() {
    let mut view = showing(&["1", "2"]);
    view.confirm(100);
    view.open("sent");
    assert_eq!(view.offset(), 0);
    view.open("inbox");
    assert_eq!(view.offset(), 2);
    assert_eq!(view.update(false, Vec::new(), Vec::new()).updated_at, Some(100));
}

#[test]
fn a_folder_never_confirmed_is_not_kept() {
    let mut view = showing(&["1"]);
    view.open("sent");
    view.open("inbox");
    assert_eq!(view.offset(), 0);
}

#[test]
fn search_results_are_not_kept_as_the_folder() {
    let mut view = showing(&["1", "2"]);
    view.confirm(100);
    assert!(!view.set_filters(Filters { keyword: "x".into(), ..Filters::default() }));
    let mark = view.begin();
    view.loaded(mark, vec![row("found")]);
    assert!(view.set_filters(Filters::default()));
    assert_eq!(view.offset(), 2);
}

#[test]
fn only_so_many_folders_are_kept() {
    let mut view = showing(&["1"]);
    view.confirm(100);
    for index in 0..=SHELVES {
        let mark = view.open(&format!("folder-{index}"));
        view.loaded(mark, vec![row("1")]);
        view.confirm(100);
    }
    view.open("sent");
    assert!(view.shelves.len() <= SHELVES);
    view.open("inbox");
    assert_eq!(view.offset(), 0, "the oldest one went");
}

#[test]
fn a_refresh_is_told_from_the_next_page() {
    let mut view = View::default();
    let mark = view.open("inbox");
    assert!(view.update(false, Vec::new(), Vec::new()).refreshing);
    view.loaded(mark, (0..PAGE).map(|i| row(&i.to_string())).collect());
    view.begin_more();
    let update = view.update(false, Vec::new(), Vec::new());
    assert!(update.loading && !update.refreshing);
}

#[test]
fn nothing_is_read_ahead_from_spam_or_a_search() {
    let mut view = showing(&["1", "2", "3"]);
    assert_eq!(view.likely_next(Some("1")), ["2", "3"]);
    assert_eq!(view.likely_next(None).len(), 3);
    view.set_filters(Filters { unread: true, ..Filters::default() });
    assert!(view.likely_next(None).is_empty());
    view.open("spam");
    assert!(view.likely_next(None).is_empty());
}

#[test]
fn a_page_travels_on_its_own() {
    let mut view = View::default();
    let mark = view.open("inbox");
    view.loaded(mark, (0..PAGE).map(|i| row(&i.to_string())).collect());
    let mark = view.begin_more().unwrap();
    view.appended(mark, vec![row("0"), row("new")]);
    let update = view.update_with_added();
    assert!(update.rows.is_none());
    assert_eq!(update.added.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["new"]);
    assert!(view.update_with_added().added.is_empty());
}

fn page(from: u64, count: u64) -> Vec<MessageMeta> {
    (from..from + count).map(|at| MessageMeta { date: 1_000_000 - at, ..row(&format!("m{at}")) }).collect()
}

fn loaded_pages(pages: u64) -> View {
    let mut view = View::default();
    let mark = view.open("inbox");
    assert!(view.loaded(mark, page(0, PAGE)));
    for number in 1..pages {
        let mark = view.begin_more().unwrap();
        assert!(view.appended(mark, page(number * PAGE, PAGE)));
    }
    view
}

#[test]
fn a_reload_keeps_the_pages_scrolled_to() {
    let mut view = loaded_pages(3);
    let mark = view.begin();
    assert!(view.loaded(mark, page(0, PAGE)));
    assert_eq!(view.offset(), 3 * PAGE);
    assert!(view.update(false, vec![], vec![]).has_more);
}

#[test]
fn new_mail_goes_on_top_of_a_long_list_and_is_told_apart() {
    let mut view = loaded_pages(2);
    let mark = view.begin();
    let mut first = vec![MessageMeta { date: 2_000_000, ..row("new") }];
    first.extend(page(0, PAGE - 1));
    assert!(view.loaded(mark, first));
    assert_eq!(view.update(true, vec![], vec![]).fresh, ["new"]);
    assert_eq!(listed(&view).first().map(String::as_str), Some("new"));
    assert_eq!(view.offset(), 2 * PAGE + 1);
    let unique: std::collections::HashSet<String> = listed(&view).into_iter().collect();
    assert_eq!(unique.len() as u64, view.offset());
}

#[test]
fn the_first_load_of_a_folder_shows_nothing_as_new() {
    let view = loaded_pages(1);
    assert!(view.update(true, vec![], vec![]).fresh.is_empty());
}

#[test]
fn a_short_answer_replaces_the_list() {
    let mut view = loaded_pages(2);
    let mark = view.begin();
    assert!(view.loaded(mark, page(0, 3)));
    assert_eq!(view.offset(), 3);
    assert!(!view.update(false, vec![], vec![]).has_more);
}

#[test]
fn a_search_is_not_mixed_with_the_folder() {
    let mut view = loaded_pages(2);
    view.set_filters(Filters { keyword: "x".to_string(), ..Filters::default() });
    let mark = view.begin();
    assert!(view.loaded(mark, page(0, PAGE)));
    assert_eq!(view.offset(), PAGE);
    assert!(view.update(true, vec![], vec![]).fresh.is_empty());
}

#[test]
#[cfg_attr(checked_memory, ignore = "a measure of speed, which the address sanitizer slows several times over")]
fn paging_stays_quick_with_a_very_long_list() {
    let mut view = loaded_pages(1);
    let mark = view.begin_more().unwrap();
    assert!(view.appended(mark, page(PAGE, 200_000)));
    let started = std::time::Instant::now();
    let mark = view.begin_more().unwrap();
    assert!(view.appended(mark, page(PAGE + 200_000, PAGE)));
    assert!(started.elapsed() < std::time::Duration::from_millis(250), "{:?}", started.elapsed());
    assert_eq!(view.offset(), 2 * PAGE + 200_000);
}

#[test]
fn a_scan_continues_from_what_it_read_not_what_it_kept() {
    let mut view = View::default();
    let mark = view.open("inbox");
    view.set_filters(Filters { keyword: "/x/".to_string(), ..Filters::default() });
    let _ = mark;
    let mark = view.begin();
    assert!(view.loaded(mark, page(0, 2)));
    view.scanned(150, true);
    assert_eq!(view.offset(), 150);
    assert!(view.update(false, vec![], vec![]).has_more);
    let mark = view.begin();
    assert!(view.loaded(mark, page(0, 3)));
    assert_eq!(view.offset(), 3);
}

#[test]
fn conversations_do_not_mix_with_the_messages_before_them() {
    let mut view = View::default();
    let mark = view.open("inbox");
    let messages: Vec<MessageMeta> =
        (0..PAGE).map(|at| MessageMeta { date: 1_000_000 - at, ..row(&format!("m{at}")) }).collect();
    assert!(view.loaded(mark, messages));
    let mark = view.begin();
    let conversations: Vec<MessageMeta> =
        (0..PAGE).map(|at| MessageMeta { date: 2_000_000 - at, messages: 2, ..row(&format!("c{at}")) }).collect();
    assert!(view.loaded(mark, conversations));
    assert!(view.row("m0").is_none());
    assert!(view.row("c0").is_some());
}
