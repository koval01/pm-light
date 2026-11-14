use super::*;

fn person(name: Option<&str>, email: &str) -> Address {
    Address { name: name.map(str::to_string), email: email.to_string() }
}

#[test]
fn a_long_list_of_people_says_how_many_are_left() {
    let list: Vec<Address> = (0..7).map(|at| person(None, &format!("p{at}@x.org"))).collect();
    let (_, left) = people(&list);
    assert_eq!(left, 3);
    let (_, left) = people(&list[..2]);
    assert_eq!(left, 0);
}

#[test]
fn a_name_stands_for_the_address_but_the_address_is_what_is_kept() {
    use slint::Model;
    let (rows, _) = people(&[person(Some("Ann"), "ann@x.org"), person(None, "bob@x.org")]);
    let first = rows.row_data(0).unwrap_or_default();
    assert_eq!(first.text, "Ann");
    assert_eq!(first.email, "ann@x.org");
    assert_eq!(rows.row_data(1).unwrap_or_default().text, "bob@x.org");
}
