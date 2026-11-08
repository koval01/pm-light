use super::*;
use crate::compose::{SendOptions, compose_from, compose_prepared, tests::message_with_body, to_outgoing};

fn labels() -> ComposeLabels {
    ComposeLabels {
        date_time: "1/1/2030, 10:00".into(),
        forwarded: "Forwarded message".into(),
        from: "From".into(),
        date: "Date".into(),
        subject: "Subject".into(),
        to: "To".into(),
        proton_footer: String::new(),
    }
}

fn own(signature: &str) -> Vec<OwnAddress> {
    vec![OwnAddress {
        id: "a1".into(),
        email: "me@x.io".into(),
        display_name: "Me".into(),
        signature: signature.into(),
        signature_text: String::new(),
    }]
}

fn state_for(mode: ComposeMode, m: Option<&Message>, signature: &str) -> ComposeState {
    let own = own(signature);
    into_plain(compose_from(mode, m, &own, &labels()), mode, m, &own, &labels())
}

#[test]
fn a_new_letter_leaves_room_above_the_signature() {
    let state = state_for(ComposeMode::New, None, "<div>Regards,<br>Me</div>");
    assert!(state.plain);
    assert_eq!(state.body, "\n\n\n\nRegards,\nMe\n\n");
}

#[test]
fn proton_s_line_reads_as_text_in_a_plain_letter() {
    let signature = crate::compose::with_footer("", "Sent with {link} secure email.");
    let state = state_for(ComposeMode::New, None, &signature);
    assert_eq!(state.body, "\n\n\n\nSent with Proton Mail secure email.\n\n");
}

#[test]
fn a_new_letter_without_a_signature_is_only_room() {
    assert_eq!(state_for(ComposeMode::New, None, "").body, "\n\n\n\n");
}

#[test]
fn a_reply_quotes_every_line() {
    let m = message_with_body("one\r\ntwo");
    let state = state_for(ComposeMode::Reply, Some(&m), "");
    assert!(state.body.starts_with("\n\n\n\n1/1/2030, 10:00, "));
    assert!(state.body.ends_with(":\n\n> one\n> two"));
    assert!(state.parent_id.is_some());
}

#[test]
fn a_forward_lists_what_it_forwards() {
    let m = message_with_body("one");
    let state = state_for(ComposeMode::Forward, Some(&m), "");
    assert!(state.body.contains("---------- Forwarded message ----------\nFrom: "));
    assert!(state.body.ends_with("\n\n> one"));
}

#[test]
fn markup_in_an_answered_mail_becomes_its_text() {
    let m = Message {
        html: true,
        safe_body: "<p>one &amp; <b>two</b></p><script>x</script>".into(),
        ..message_with_body("")
    };
    assert!(state_for(ComposeMode::Reply, Some(&m), "").body.ends_with("> one & two"));
}

#[test]
fn a_saved_draft_comes_back_exactly() {
    let m = message_with_body("\n\n\n\nmy words  \n> kept");
    assert_eq!(state_for(ComposeMode::Draft, Some(&m), "unused").body, "\n\n\n\nmy words  \n> kept");
}

#[test]
fn a_prepared_letter_loses_its_markup() {
    let state = prepared(compose_prepared(&[], "S", "a <b>\nc"));
    assert!(state.plain);
    assert_eq!(state.body, "a <b>\nc");
}

#[test]
fn the_signature_is_swapped_where_it_stands() {
    let body = "\n\n\n\nwords\n\nOld sig\n\n> quote";
    assert_eq!(replace_signature(body, "<div>Old sig</div>", "New sig"), "\n\n\n\nwords\n\nNew sig\n\n> quote");
}

#[test]
fn a_first_signature_goes_to_the_end() {
    assert_eq!(replace_signature("words", "", "New"), "words\n\nNew");
    assert_eq!(replace_signature("words", "", ""), "words");
}

#[test]
fn a_letter_says_what_it_is() {
    let plain = ComposeState { to: "a@x.io".into(), plain: true, ..compose_prepared(&[], "", "") };
    let rich = ComposeState { plain: false, ..plain.clone() };
    assert_eq!(to_outgoing(&plain, SendOptions::default()).unwrap().mime, "text/plain");
    assert_eq!(to_outgoing(&rich, SendOptions::default()).unwrap().mime, "text/html");
}
