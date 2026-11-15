use super::*;

fn arguments(list: &[&str]) -> impl Iterator<Item = String> {
    list.iter().map(|argument| (*argument).to_string()).collect::<Vec<_>>().into_iter()
}

#[test]
fn the_port_is_read_from_its_argument_only() {
    assert_eq!(port_in(arguments(&["--soak-minutes=10", "--soak-link-port=40123"])), Some(40123));
    assert_eq!(port_in(arguments(&["--soak-minutes=10"])), None);
    for wrong in [
        "--soak-link-port=",
        "--soak-link-port=0",
        "--soak-link-port=70000",
        "--soak-link-port=-1",
        "--soak-link-port=1x",
    ] {
        assert_eq!(port_in(arguments(&[wrong])), None, "{wrong} was taken for a port");
    }
}

#[test]
fn the_made_up_mail_is_one_link_cleaned_as_any_mail() {
    let page = page(40123);
    assert!(page.contains("http://127.0.0.1:40123/link-check"), "the link lost its address: {page}");
    // As every link of a mail: it asks for a window of its own, which the mail view hands to the browser.
    assert!(page.contains("target=\"_blank\""), "the link was not cleaned as a mail's: {page}");
    assert_eq!(page.matches("<a ").count(), 1, "more than one link");
    assert!(page.len() > 5_000, "too short to fill the body");
}
