use super::*;
use proton_core::compose;

fn state(body: &str, plain: bool) -> ComposeState {
    ComposeState { body: body.into(), plain, ..compose::compose_prepared(&[], "", "") }
}

fn area_for(body: &str, plain: bool) -> String {
    writing_area(&state(body, plain), false)
}

#[test]
fn a_plain_letter_keeps_the_lines_it_starts_with() {
    // One more than the letter has: the parser takes it away again.
    assert!(area_for("\n\n\n\nwords", true).contains(">\n\n\n\n\nwords</textarea>"));
}

#[test]
fn a_plain_letter_cannot_close_its_field() {
    let area = area_for("</textarea><script>x</script>", true);
    assert_eq!(area.matches("</textarea>").count(), 1);
    assert!(!area.contains("<script>"));
}

#[test]
fn a_rich_letter_stays_in_the_editor() {
    assert!(area_for("<b>words</b>", false).contains(r#"contenteditable="true"><b>words</b></div>"#));
}

/// What the editor is made again from, once the engine behind it ended: the letter as Rust holds it, with the script
/// that reports it, under a fresh nonce.
#[test]
fn a_page_made_again_holds_the_letter_as_last_reported() {
    let rich = page_for(&state("<p>Dear <b>all</b></p>", false), false, false);
    assert!(rich.contains(r#"contenteditable="true"><p>Dear <b>all</b></p></div>"#));
    assert!(rich.contains("window.ipc.postMessage"), "without its script the page made again would report nothing");
    let plain = page_for(&state("line one\nline two", true), false, true);
    assert!(plain.contains(">\nline one\nline two</textarea>"));
    let nonce = |page: &str| page.split("'nonce-").nth(1).and_then(|rest| rest.split('\'').next()).map(str::to_string);
    assert_ne!(nonce(&rich), nonce(&plain), "every page has a nonce of its own");
}

#[test]
fn a_letter_served_again_has_a_new_address_and_only_it_loads() {
    let first = serve("one");
    let second = serve("two");
    assert_ne!(first, second, "a page cached under the old address must not stand in for the new one");
    assert!(own_page(&second));
    assert!(!own_page(&first), "the page from before the end loads no more");
}

#[test]
fn every_editor_page_has_a_nonce_of_its_own() {
    let nonce =
        |page: String| page.split("'nonce-").nth(1).and_then(|rest| rest.split('\'').next()).unwrap().to_string();
    let first = nonce(page("", false));
    assert_eq!(first.len(), 32);
    assert_ne!(first, "0".repeat(32));
    assert_ne!(first, nonce(page("", false)));
}
