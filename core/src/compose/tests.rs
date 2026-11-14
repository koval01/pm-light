use super::*;
use crate::{
    crypto::SignatureStatus,
    details::details_of,
    model::{MailProtection, MessageMeta},
};

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

fn address(email: &str) -> Address {
    Address { name: None, email: email.into() }
}

fn message() -> Message {
    Message {
        meta: MessageMeta {
            id: "m1".into(),
            folder: "inbox".into(),
            from: Address { name: Some("Ann".into()), email: "ann@x.io".into() },
            to: vec![address("me@proton.me"), address("bob@x.io")],
            subject: "Hi".into(),
            snippet: String::new(),
            date: 0,
            unread: false,
            starred: false,
            labels: vec![],
            attachment_count: 0,
            conversation_id: "c".into(),
            messages: 0,
            expires: 0,
        },
        address_id: "a1".into(),
        size: 0,
        signature: SignatureStatus::Unsigned,
        raw_headers: String::new(),
        unsubscribe: None,
        receipt_requested: false,
        cc: vec![address("carol@x.io")],
        bcc: vec![],
        reply_to: vec![],
        body: "line1\nline2".into(),
        safe_body: String::new(),
        protection: MailProtection::default(),
        details: details_of(""),
        html: false,
        attachments: vec![],
        from_proton: Some(false),
    }
}

pub(in crate::compose) fn message_with_body(body: &str) -> Message {
    Message { body: body.into(), ..message() }
}

fn own() -> Vec<OwnAddress> {
    let one = |id: &str, email: &str, signature: &str| OwnAddress {
        id: id.into(),
        email: email.into(),
        display_name: String::new(),
        signature: signature.into(),
        signature_text: String::new(),
    };
    vec![
        one("a1", "me@proton.me", ""),
        one("a2", "alias@proton.me", ""),
        one("a3", "me@myblog.com", "<div>Kind regards,<br>the author</div>"),
    ]
}

fn start(mode: ComposeMode, m: &Message) -> ComposeState {
    compose_from(mode, Some(m), &own(), &labels())
}

#[test]
fn parses_separators_names_and_duplicates() {
    assert_eq!(parse_addresses("a@b.c, Name <d@e.f>; g@h.i\nA@B.C"), ["a@b.c", "d@e.f", "g@h.i"]);
    assert!(parse_addresses(" , ").is_empty());
}

#[test]
fn checks_addresses() {
    for good in ["a@b.co", "a.b+c@d.e.fr"] {
        assert!(is_valid_address(good));
    }
    for bad in ["a", "a@b", "a b@c.d", "@b.c"] {
        assert!(!is_valid_address(bad));
    }
}

#[test]
fn a_reply_goes_to_the_sender_threaded_with_a_quote() {
    let c = start(ComposeMode::Reply, &message());
    assert_eq!((c.to.as_str(), c.cc.as_str(), c.subject.as_str()), ("ann@x.io", "", "Re: Hi"));
    assert_eq!((c.parent_id.as_deref(), c.action), (Some("m1"), Some(0)));
    assert!(c.body.contains("<blockquote><p>line1<br>"));
}

#[test]
fn prefers_reply_to() {
    let m = Message { reply_to: vec![address("list@x.io")], ..message() };
    assert_eq!(start(ComposeMode::Reply, &m).to, "list@x.io");
}

#[test]
fn reply_all_keeps_everyone_but_us() {
    let c = start(ComposeMode::ReplyAll, &message());
    assert_eq!((c.to.as_str(), c.cc.as_str(), c.action), ("ann@x.io", "bob@x.io, carol@x.io", Some(1)));
}

#[test]
fn replying_to_our_own_mail_continues_to_its_recipients() {
    let mut m = message();
    m.meta.from = address("me@proton.me");
    assert_eq!(start(ComposeMode::Reply, &m).to, "me@proton.me, bob@x.io");
}

#[test]
fn does_not_stack_prefixes() {
    let with = |subject: &str| {
        let mut m = message();
        m.meta.subject = subject.into();
        m
    };
    assert_eq!(start(ComposeMode::Reply, &with("RE: Hi")).subject, "RE: Hi");
    assert_eq!(start(ComposeMode::Forward, &with("Fw: Hi")).subject, "Fw: Hi");
    assert_eq!(start(ComposeMode::Forward, &with("Fwd: Hi")).subject, "Fwd: Hi");
}

#[test]
fn quotes_html_from_the_cleaned_body_never_the_raw_one() {
    let m = Message { html: true, body: "<script>x</script>".into(), safe_body: "<p>one</p>".into(), ..message() };
    let body = start(ComposeMode::Reply, &m).body;
    assert!(body.contains("<blockquote><p>one</p>") && !body.contains("<script>"));
}

#[test]
fn a_sender_name_cannot_inject_markup() {
    let mut m = message();
    m.meta.from.name = Some("<img src=x onerror=y>".into());
    assert!(!start(ComposeMode::Reply, &m).body.contains("<img"));
}

#[test]
fn a_forward_has_no_recipients() {
    let c = start(ComposeMode::Forward, &message());
    assert_eq!((c.to.as_str(), c.action, c.subject.as_str()), ("", Some(2), "Fw: Hi"));
    assert!(c.body.contains("Subject: Hi"));
}

#[test]
fn a_draft_restores_its_fields_and_attachments() {
    let file = Attachment {
        id: "a".into(),
        name: "f".into(),
        size: 1,
        mime: "x".into(),
        key_packets: "k".into(),
        signed: true,
        inline: false,
        content_id: None,
        previewable: false,
    };
    let m = Message { attachments: vec![file.clone()], bcc: vec![address("z@x.io")], ..message() };
    let c = start(ComposeMode::Draft, &m);
    assert_eq!((c.draft_id.as_deref(), c.bcc.as_str(), c.subject.as_str()), (Some("m1"), "z@x.io", "Hi"));
    assert_eq!(c.existing, [file]);
}

#[test]
fn a_new_message_is_empty() {
    assert_eq!(compose_from(ComposeMode::New, None, &own(), &labels()).to, "");
}

#[test]
fn answers_from_the_address_the_mail_came_to_with_its_signature() {
    let m = Message { address_id: "a3".into(), ..message() };
    let c = start(ComposeMode::Reply, &m);
    assert_eq!(c.from.as_deref(), Some("me@myblog.com"));
    assert!(c.body.contains("Kind regards"));
}

#[test]
fn a_signature_is_swapped_in_place() {
    let body = compose_from(ComposeMode::New, None, &[own()[2].clone()], &labels()).body;
    let swapped = replace_signature(&body, "<div>Another</div>");
    assert!(swapped.contains("Another") && !swapped.contains("regards"));
    assert_eq!(swapped.matches(SIGNATURE_CLASS).count(), 1);
}

#[test]
fn a_signature_is_removed_for_an_address_without_one() {
    let body = compose_from(ComposeMode::New, None, &[own()[2].clone()], &labels()).body;
    assert!(!replace_signature(&body, "").contains(SIGNATURE_CLASS));
}

#[test]
fn a_signature_is_added_when_there_was_none() {
    let swapped = replace_signature("<div>text</div>", "<div>New</div>");
    assert!(swapped.contains("New") && swapped.contains("text"));
}

#[test]
fn typed_text_survives_a_signature_swap() {
    let body = format!(r#"<div>my words</div><div class="{SIGNATURE_CLASS}">old</div>"#);
    assert!(replace_signature(&body, "new").contains("my words"));
}

#[test]
fn prepared_mail_cannot_inject_markup() {
    let c = compose_prepared(&["a@x.io".into()], "S", "<b>x</b>");
    assert_eq!((c.to.as_str(), c.body.as_str()), ("a@x.io", "<div>&lt;b&gt;x&lt;/b&gt;</div>"));
}

#[test]
fn prepared_mail_keeps_its_lines() {
    let c = compose_prepared(&[], "S", "one\r\ntwo\nthree");
    assert_eq!(c.body, "<div>one<br>two<br>three</div>");
}

#[test]
fn a_letter_starts_as_the_account_asks() {
    let habits =
        Habits { font_face: Some("Georgia".into()), font_size: Some(16), right_to_left: true, ..Habits::default() };
    assert_eq!(
        styled_first_line("<div><br></div><p>rest</p>", &habits),
        r#"<div style="font-family: Georgia; font-size: 16px;" dir="rtl"><br></div><p>rest</p>"#
    );
}

#[test]
fn a_letter_stays_plain_when_nothing_is_asked() {
    assert_eq!(styled_first_line("<div><br></div>", &Habits::default()), "<div><br></div>");
    assert_eq!(
        styled_first_line("<p>a draft</p>", &Habits { right_to_left: true, ..Habits::default() }),
        "<p>a draft</p>"
    );
}

#[test]
fn a_draft_with_no_subject_stays_without_one() {
    let m = |subject: &str| Message { meta: MessageMeta { subject: subject.into(), ..message().meta }, ..message() };
    let subject = |m: &Message| compose_from(ComposeMode::Draft, Some(m), &own(), &labels()).subject;
    assert_eq!(subject(&m("(No Subject)")), "");
    assert_eq!(subject(&m("Hi")), "Hi");
}

#[test]
fn proton_s_line_stands_under_the_signature() {
    let signed = with_footer("<div>Me</div>", "Sent with {link} secure email.");
    assert!(signed.starts_with("<div>Me</div><div><br></div><div>Sent with <a href=\"https://proton.me/mail/home\""));
    assert!(signed.ends_with(">Proton Mail</a> secure email.</div>"));
}

#[test]
fn proton_s_line_needs_no_signature() {
    assert!(with_footer("", "Sent with {link}.").starts_with("<div>Sent with <a "));
}

#[test]
fn words_for_the_line_cannot_become_markup() {
    assert!(with_footer("", "<b>{link}").starts_with("<div>&lt;b&gt;<a "));
}

fn state(to: &str) -> ComposeState {
    ComposeState { to: to.into(), ..ComposeState::empty(ComposeMode::New) }
}

#[test]
fn sending_stops_at_an_invalid_address_by_name() {
    let Err(Error::InvalidAddress(address)) = to_outgoing(&state("good@x.io, bad"), SendOptions::default()) else {
        panic!("expected a refusal");
    };
    assert_eq!(address, "bad");
}

#[test]
fn a_send_carries_its_options() {
    let options = SendOptions { deliver_at: Some(9), password: Some("p".into()), ..Default::default() };
    let mail = to_outgoing(&state("a@x.io"), options).unwrap();
    assert_eq!((mail.deliver_at, mail.password.as_deref(), mail.mime.as_str()), (Some(9), Some("p"), "text/html"));
}

#[test]
fn an_empty_password_means_no_password() {
    let options = SendOptions { password: Some(String::new()), ..Default::default() };
    assert_eq!(to_outgoing(&state("a@x.io"), options).unwrap().password, None);
}

#[test]
fn autosave_keeps_what_is_valid_and_never_fails() {
    assert_eq!(draft_outgoing(&state("a@x.io, half@typ")).to, ["a@x.io"]);
}

fn person(name: &str, email: &str) -> ContactEmail {
    ContactEmail { name: name.to_string(), email: email.to_string(), contact_id: String::new() }
}

#[test]
fn a_space_separates_typed_recipients() {
    assert_eq!(typed_recipients("a@b.org c@d.org,e@f.org"), ["a@b.org", "c@d.org", "e@f.org"]);
    assert_eq!(typed_recipients("A@b.org a@B.org"), ["A@b.org"]);
    assert!(typed_recipients("  ").is_empty());
}

#[test]
fn suggestions_put_the_address_before_the_name() {
    let contacts = [person("Mark", "zed@example.org"), person("Other", "mark@example.org")];
    let found = suggestions(&contacts, "MAR", &[], 5);
    assert_eq!(found[0].email, "mark@example.org");
    assert_eq!(found[1].email, "zed@example.org");
}

#[test]
fn suggestions_skip_what_is_already_there() {
    let contacts = [person("", "mark@example.org"), person("", "mary@example.org")];
    let found = suggestions(&contacts, "mar", &["MARK@example.org".to_string()], 5);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].email, "mary@example.org");
    assert!(suggestions(&contacts, " ", &[], 5).is_empty());
    assert_eq!(suggestions(&contacts, "mar", &[], 1).len(), 1);
}

#[test]
fn only_ascii_addresses_pass() {
    assert!(is_valid_address("first.last+tag@sub.example.org"));
    assert!(is_valid_address("a@xn--e1aybc.xn--p1ai"));
    for bad in [
        "\u{44f}\u{449}\u{438}\u{43a}@example.org",
        "user@\u{43f}\u{440}\u{438}\u{43c}\u{435}\u{440}.\u{440}\u{444}",
        "\u{444}\u{444}\u{444}2004@\u{444}\u{444}\u{444}.\u{444}\u{444}\u{444}",
        "a@b",
        "a@-b.org",
        "a@b-.org",
        "a@b..org",
        "a@b.1",
        "@b.org",
    ] {
        assert!(!is_valid_address(bad), "{bad}");
    }
}

#[test]
fn a_domain_is_read_from_a_valid_address() {
    assert_eq!(domain_of("Me@Example.ORG").as_deref(), Some("example.org"));
    assert_eq!(domain_of("not an address"), None);
}
