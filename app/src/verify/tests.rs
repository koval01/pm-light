use super::*;

fn parsed(url: &str) -> Url {
    Url::parse(url).unwrap_or_else(|_| panic!("{url}"))
}

fn outcome(url: &str) -> Option<Outcome> {
    outcome_of(&parsed(url))
}

#[test]
fn a_captcha_needs_no_token_of_its_own() {
    let solved = outcome("proton-mail-light-verify://solved?token=abc&type=captcha");
    assert_eq!(solved, Some(Outcome::Solved(None)));
}

#[test]
fn other_methods_hand_back_their_token() {
    let solved = outcome("proton-mail-light-verify://solved?token=a+b&type=email");
    assert_eq!(solved, Some(Outcome::Solved(Some(("a b".to_string(), "email".to_string())))));
}

#[test]
fn the_page_can_ask_to_leave() {
    assert_eq!(outcome("proton-mail-light-verify://closed"), Some(Outcome::Closed));
}

#[test]
fn ordinary_addresses_are_no_outcome() {
    assert_eq!(outcome("https://verify.proton.me/?token=abc&type=captcha"), None);
}

#[test]
fn opens_only_proton_s_page() {
    assert!(page_url("https://verify.proton.me/?methods=captcha&token=t", "ru", false).is_some());
    assert!(page_url("http://verify.proton.me/", "ru", false).is_none());
    assert!(page_url("https://verify.proton.me.evil.example/", "ru", false).is_none());
    assert!(page_url("not an address", "ru", false).is_none());
}

#[test]
fn keeps_the_challenge_and_adds_language_and_theme() {
    let url = page_url("https://verify.proton.me/?methods=captcha&token=t", "de", true);
    assert_eq!(url.as_ref().and_then(Url::query), Some("methods=captcha&token=t&locale=de&theme=1"));
}

#[test]
fn only_proton_s_own_pages_may_stay_on_screen() {
    let proton = |url: &str| is_proton(&parsed(url));
    assert!(proton("https://verify.proton.me/") && proton("https://proton.me/legal"));
    assert!(!proton("https://proton.me.evil.example/") && !proton("https://evilproton.me/"));
    assert!(!proton("http://verify.proton.me/") && !proton("about:blank"));
}

#[test]
fn loads_the_web_and_nothing_else() {
    let allowed = |url: &str| may_load(&parsed(url));
    assert!(allowed("https://newassets.hcaptcha.com/captcha") && allowed("about:blank"));
    assert!(!allowed("file:///etc/passwd") && !allowed("mailto:a@b.c"));
}
