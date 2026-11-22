use super::*;
use proton_core::{
    categories::{Categories, from_labels},
    model::Count,
};
use serde_json::json;

fn account(shown: &[&str]) -> Categories {
    let labels: Vec<_> =
        categories::ALL.iter().map(|(id, _)| json!({"ID": id, "Display": shown.contains(id)})).collect();
    Categories { allowed: true, chosen: true, list: from_labels(&json!({ "Labels": labels })) }
}

fn counts(unread: &[(&str, u64)]) -> Counts {
    unread
        .iter()
        .map(|(id, unread)| ((*id).to_string(), Count { total: unread.saturating_mul(2), unread: *unread }))
        .collect()
}

#[test]
fn every_tab_shown_counts_its_own_unread_mail() {
    let tabs = tabs(&account(&["24", "20", "25"]), &counts(&[("24", 7), ("20", 2), ("25", 1), ("21", 4)]));
    let shown: Vec<(String, String, i32)> =
        tabs.iter().map(|tab| (tab.id.to_string(), tab.name.to_string(), tab.unread)).collect();
    // Promotions is hidden, so its four unread count in Primary.
    assert_eq!(
        shown,
        [
            ("24".into(), "primary".into(), 11),
            ("20".into(), "social".into(), 2),
            ("25".into(), "newsletters".into(), 1)
        ]
    );
}

#[test]
fn no_tabs_where_the_inbox_is_one_list() {
    let mut off = account(&["24", "20"]);
    off.chosen = false;
    assert!(tabs(&off, &counts(&[("24", 3)])).is_empty());
    let mut refused = account(&["24", "20"]);
    refused.allowed = false;
    assert!(tabs(&refused, &counts(&[("24", 3)])).is_empty());
}

#[test]
fn a_count_too_big_for_the_window_stays_at_its_most() {
    let tabs = tabs(&account(&["24"]), &counts(&[("24", u64::MAX)]));
    assert_eq!(tabs.first().map(|tab| tab.unread), Some(i32::MAX));
}
