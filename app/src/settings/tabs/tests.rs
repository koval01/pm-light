use super::*;

fn same(key: &str) -> String {
    key.to_string()
}

#[test]
fn every_tab_is_shown_with_no_query() {
    assert_eq!(shown(&same, "").len(), all().count());
}

#[test]
fn a_heading_stands_only_above_the_first_of_a_group() {
    let rows = shown(&same, "");
    assert_eq!(rows.iter().filter(|row| !row.heading.is_empty()).count(), GROUPS.len());
}

#[test]
fn a_search_also_looks_in_what_a_tab_is_about() {
    let rows = shown(&same, "signOut");
    assert_eq!(rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["overview"]);
}

#[test]
fn tab_ids_are_not_repeated() {
    let mut ids: Vec<&str> = all().map(|tab| tab.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all().count());
}
