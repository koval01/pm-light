use super::*;

fn contact(name: &str, email: &str) -> ContactEmail {
    ContactEmail { name: name.to_string(), email: email.to_string(), contact_id: String::new() }
}

#[test]
fn a_search_looks_at_the_name_and_the_address() {
    let contacts = [contact("Ann Lee", "ann@example.org"), contact("Bob", "bob@work.net")];
    assert_eq!(wanted(&contacts, "lee"), [0]);
    assert_eq!(wanted(&contacts, "WORK"), [1]);
    assert_eq!(wanted(&contacts, " "), [0, 1]);
    assert_eq!(wanted(&contacts, "zed"), [] as [usize; 0]);
}
