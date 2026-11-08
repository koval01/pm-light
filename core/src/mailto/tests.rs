use super::*;

#[test]
fn reads_a_plain_address() {
    assert_eq!(parse("mailto:a@example.com").unwrap().to, ["a@example.com"]);
}

#[test]
fn reads_the_scheme_in_any_case() {
    assert!(parse("MAILTO:a@example.com").is_some());
}

#[test]
fn reads_several_recipients_a_subject_and_a_body() {
    let link = parse("mailto:a@x.io,b@x.io?to=c@x.io&subject=Hi%20there&body=Line%0Atwo").unwrap();
    assert_eq!(link.to, ["a@x.io", "b@x.io", "c@x.io"]);
    assert_eq!(link.subject, "Hi there");
    assert_eq!(link.body, "Line\ntwo");
}

#[test]
fn a_link_may_have_no_recipient() {
    assert!(parse("mailto:?subject=x").unwrap().to.is_empty());
}

#[test]
fn ignores_everything_else() {
    assert_eq!(parse("https://example.com"), None);
    assert_eq!(parse("not a link"), None);
}

#[test]
fn asks_for_nothing_but_a_letter() {
    let link = parse("mailto:a@x.io?attach=/etc/passwd&attachment=~/.ssh/id_rsa&bcc=b@x.io").unwrap();
    assert_eq!(link, Mailto { to: vec!["a@x.io".into()], ..Default::default() });
}

#[test]
fn a_huge_link_is_refused() {
    let link = format!("mailto:a@x.io?body={}", "x".repeat(LONGEST));
    assert_eq!(parse(&link), None);
}
