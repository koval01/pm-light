use super::*;
use crate::model::Count;
use serde_json::json;

/// Every category as Proton lists it among the system folders, with these shown as tabs.
fn labels(shown: &[&str]) -> Value {
    let labels: Vec<Value> =
        ALL.iter().map(|(id, _)| json!({"ID": id, "Name": "x", "Display": i32::from(shown.contains(id))})).collect();
    json!({ "Code": 1000, "Labels": labels })
}

fn account(shown: &[&str]) -> Categories {
    Categories { allowed: true, chosen: true, list: from_labels(&labels(shown)) }
}

fn counts(unread: &[(&str, u64)]) -> Counts {
    unread.iter().map(|(id, unread)| ((*id).to_string(), Count { total: 100, unread: *unread })).collect()
}

#[test]
fn the_categories_come_in_protons_order_whatever_order_they_are_listed_in() {
    let mut reply = labels(&["24", "20"]);
    if let Some(list) = reply.get_mut("Labels").and_then(Value::as_array_mut) {
        list.reverse();
        list.push(json!({"ID": "5", "Display": 1}));
        list.push(json!({"ID": "23", "Display": 1}));
    }
    let ids: Vec<&str> = from_labels(&reply).iter().map(|category| category.id).collect();
    assert_eq!(ids, ["24", "20", "21", "25", "26", "22"]);
}

#[test]
fn display_is_read_as_proton_sends_it() {
    let reply = json!({"Labels": [
        {"ID": "24", "Display": 1}, {"ID": "20", "Display": true}, {"ID": "21", "Display": 0},
        {"ID": "25", "Display": false}, {"ID": "26"}, {"ID": "22", "Display": "1"},
    ]});
    let shown: Vec<bool> = from_labels(&reply).iter().map(|category| category.shown).collect();
    assert_eq!(shown, [true, true, false, false, false, false]);
}

#[test]
fn a_hostile_or_empty_answer_has_no_categories() {
    for reply in [json!(null), json!({}), json!({"Labels": "24"}), json!({"Labels": [1, null, {"ID": 24}]})] {
        assert!(from_labels(&reply).is_empty(), "{reply}");
    }
}

#[test]
fn tabs_are_primary_and_what_is_shown() {
    let account = account(&["20", "25"]);
    let tabs: Vec<&str> = account.tabs().iter().map(|tab| tab.id).collect();
    // Primary cannot be hidden in Proton's client, so it has a tab even where it says otherwise.
    assert_eq!(tabs, ["24", "20", "25"]);
    assert!(account.has_tab("25") && account.has_tab("24"));
    assert!(!account.has_tab("21"), "a hidden category has no tab");
    assert!(!account.has_tab("0") && !account.has_tab("23") && !account.has_tab(""));
}

#[test]
fn categories_are_on_only_where_both_sides_say_so_and_primary_is_there() {
    assert!(account(&["24"]).on());
    let mut off = account(&["24"]);
    off.chosen = false;
    assert!(!off.on() && !off.has_tab("24"));
    let mut refused = account(&["24"]);
    refused.allowed = false;
    assert!(!refused.on());
    let without_primary =
        Categories { allowed: true, chosen: true, list: from_labels(&json!({"Labels": [{"ID": "20", "Display": 1}]})) };
    assert!(!without_primary.on());
    assert!(!Categories::default().on());
}

#[test]
fn primary_asks_for_the_hidden_categories_and_the_old_forum_too() {
    let some = account(&["24", "20", "25"]);
    assert_eq!(some.labels_of(PRIMARY), ["24", "21", "26", "22", "23"]);
    assert_eq!(some.labels_of("20"), ["20"]);
    assert_eq!(account(&ALL.map(|(id, _)| id)).labels_of(PRIMARY), ["24", "23"]);
}

#[test]
fn what_is_no_category_asks_for_the_whole_folder() {
    let account = account(&["24"]);
    for wrong in ["", "0", "23", "5", "24 ", "LabelID=0&LabelID[]=1"] {
        assert!(account.labels_of(wrong).is_empty(), "{wrong:?}");
    }
}

#[test]
fn primary_counts_the_hidden_categories_unread_mail_as_its_own() {
    let account = account(&["24", "20"]);
    let counts = counts(&[("24", 5), ("20", 3), ("21", 2), ("25", 1), ("0", 50), ("23", 9)]);
    // The forum is asked for, but Proton's client does not count it.
    assert_eq!(account.unread(PRIMARY, &counts), 8);
    assert_eq!(account.unread("20", &counts), 3);
    assert_eq!(account.unread("26", &counts), 0, "no count is none unread");
}

#[test]
fn a_huge_count_does_not_wrap() {
    let account = account(&["24"]);
    assert_eq!(account.unread(PRIMARY, &counts(&[("24", u64::MAX), ("20", 7)])), u64::MAX);
}

#[test]
fn counts_are_asked_as_protons_client_asks_for_them() {
    let mut categories = account(&["24"]);
    assert_eq!(categories.count_parameter(), Some(1));
    categories.chosen = false;
    assert_eq!(categories.count_parameter(), Some(0));
    categories.allowed = false;
    assert_eq!(categories.count_parameter(), None);
}

#[test]
fn the_setting_is_read_as_a_flag_or_a_number() {
    let mut categories = Categories::default();
    for (raw, on) in [
        (json!({"MailCategoryView": true}), true),
        (json!({"MailCategoryView": 1}), true),
        (json!({"MailCategoryView": false}), false),
        (json!({"MailCategoryView": 0}), false),
        (json!({"MailCategoryView": "true"}), false),
        (json!({}), false),
    ] {
        categories.read_settings(&raw);
        assert_eq!(categories.chosen, on, "{raw}");
    }
}

#[test]
fn an_organization_allows_categories_only_when_it_says_so() {
    assert!(allowed_by(&json!({"Code": 1000, "MailCategoryViewEnabled": true})));
    for settings in [json!({"MailCategoryViewEnabled": false}), json!({"MailCategoryViewEnabled": 1}), json!({})] {
        assert!(!allowed_by(&settings), "{settings}");
    }
}

#[test]
fn every_category_has_its_word() {
    for (id, name) in ALL {
        assert_eq!(name_of(id), Some(name));
    }
    assert_eq!(name_of("23"), None);
    assert_eq!(name_of("0"), None);
}

#[test]
fn a_tab_is_hidden_with_the_label_as_proton_has_it() {
    let label = json!({"ID": "21", "Name": "Promotions", "Color": "#8080FF", "Display": 1, "Notify": 0, "Order": 3});
    assert_eq!(
        shown_change(&label, false),
        json!({"Name": "Promotions", "Color": "#8080FF", "Display": 0, "Notify": 0})
    );
    assert_eq!(shown_change(&label, true).get("Display"), Some(&json!(1)));
    assert_eq!(shown_change(&json!(null), true), json!({"Name": null, "Color": null, "Display": 1, "Notify": null}));
}
