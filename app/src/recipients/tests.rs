use super::*;

fn contact(name: &str, email: &str) -> ContactEmail {
    ContactEmail { name: name.to_string(), email: email.to_string(), contact_id: String::new() }
}

#[test]
fn a_domain_without_mail_makes_its_addresses_bad() {
    let mut people = People::new("a@gone.example, b@gone.example, c@fine.example", "", "");
    assert_eq!(people.unchecked_domains(), ["gone.example", "fine.example"]);
    assert!(!people.has_bad());
    people.mark_dead("gone.example".to_string());
    assert!(people.has_bad());
    assert!(people.is_bad("B@GONE.example"));
    assert!(!people.is_bad("c@fine.example"));
}

#[test]
fn a_domain_is_asked_about_once() {
    let mut people = People::new("a@one.example", "", "");
    assert_eq!(people.unchecked_domains().len(), 1);
    people.left(TO, "b@one.example");
    assert_eq!(people.unchecked_domains(), [] as [std::string::String; 0]);
}

#[test]
fn a_space_finishes_an_address() {
    let mut people = People::default();
    assert_eq!(people.typed(TO, "a@b.org ", &[]), "");
    assert_eq!(people.list(TO), ["a@b.org"]);
    assert_eq!(people.typed(TO, "c@d", &[]), "c@d");
    assert_eq!(people.list(TO).len(), 1);
}

#[test]
fn a_paste_becomes_several_addresses() {
    let mut people = People::default();
    assert_eq!(people.typed(CC, "a@b.org, c@d.org e@f.org; g", &[]), "g");
    assert_eq!(people.list(CC), ["a@b.org", "c@d.org", "e@f.org"]);
}

#[test]
fn a_named_address_keeps_only_the_address() {
    let people = People::new("Some One <a@b.org>, c@d.org", "", "");
    assert_eq!(people.list(TO), ["a@b.org", "c@d.org"]);
}

#[test]
fn a_separator_takes_the_contact_being_typed() {
    let contacts = [contact("Anna", "anna@example.org")];
    let mut people = People::default();
    people.typed(TO, "ann", &contacts);
    people.commit(TO, "ann");
    assert_eq!(people.list(TO), ["anna@example.org"]);
}

#[test]
fn a_valid_address_is_not_replaced_by_a_contact() {
    let contacts = [contact("Anna", "anna@example.org.uk")];
    let mut people = People::default();
    people.typed(TO, "anna@example.org", &contacts);
    people.commit(TO, "anna@example.org");
    assert_eq!(people.list(TO), ["anna@example.org"]);
}

#[test]
fn leaving_the_field_never_guesses() {
    let contacts = [contact("Anna", "anna@example.org")];
    let mut people = People::default();
    people.typed(TO, "ann", &contacts);
    people.left(TO, "ann");
    assert_eq!(people.list(TO), ["ann"]);
    assert!(people.has_bad());
}

#[test]
fn an_address_is_added_once_and_removed_whole() {
    let mut people = People::new("a@b.org, c@d.org", "", "");
    people.commit(TO, "A@B.org");
    assert_eq!(people.list(TO).len(), 2);
    people.remove(TO, 0);
    assert_eq!(people.list(TO), ["c@d.org"]);
    people.remove_last(TO);
    people.remove_last(TO);
    assert!(people.is_empty());
}

#[test]
fn lines_break_before_they_overflow() {
    let list: Vec<String> = (0..6).map(|index| format!("person{index}@example.org")).collect();
    let (full, last) = lines_of(&list, 420.0);
    let all: Vec<usize> = full.iter().flatten().chain(&last).copied().collect();
    assert_eq!(all, [0, 1, 2, 3, 4, 5]);
    assert!(full.len() >= 2);
    assert!(full.iter().all(|line| !line.is_empty()));
}

#[test]
fn a_full_last_line_leaves_a_new_one_to_type_in() {
    let list = vec!["somebody-with-a-long-name@example.org".to_string()];
    let (full, last) = lines_of(&list, 320.0);
    assert_eq!(full, [vec![0]]);
    assert_eq!(last, [] as [usize; 0]);
}

#[test]
fn an_address_wider_than_the_field_still_gets_a_line() {
    let list = vec!["x".repeat(400), "a@b.org".to_string()];
    let (full, last) = lines_of(&list, 300.0);
    assert_eq!(full[0], [0]);
    assert_eq!(last, [1]);
}
