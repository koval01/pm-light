use super::*;

#[test]
fn only_the_page_served_now_is_own() {
    let served = OnePage::new("testpage");
    let first = served.serve(1, "<p>one</p>".into());
    assert!(served.is_own(&first));
    let second = served.serve(2, "<p>two</p>".into());
    assert!(served.is_own(&second));
    assert!(!served.is_own(&first), "an earlier page must not load again");
}

#[test]
fn an_earlier_page_is_still_ours_and_never_a_link() {
    let served = OnePage::new("testpage");
    let first = served.serve(1, String::new());
    let _second = served.serve(2, String::new());
    assert!(served.is_ours(&first), "an earlier page reported late went to the browser");
    let origin = served.origin();
    assert!(served.is_ours(&format!("{origin}/7")));
    // Only this protocol's own origin: anything that merely starts the same way is a link like any other.
    for other in [
        format!("{origin}.example.com/1"),
        format!("{origin}evil/1"),
        "https://example.com/testpage".to_string(),
        "http://testpage.page.example/1".to_string(),
        "testpages://page/1".to_string(),
        String::new(),
    ] {
        assert!(!served.is_ours(&other), "{other} was taken for the app's own page");
    }
}
