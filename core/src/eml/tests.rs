use super::*;
use crate::{
    crypto::SignatureStatus,
    details::details_of,
    mime::parse_mime,
    model::{Address, MailProtection, MessageMeta},
};

fn message(html: bool) -> Message {
    Message {
        meta: MessageMeta {
            id: "m".into(),
            folder: "inbox".into(),
            from: Address { name: None, email: "a@b.org".into() },
            to: vec![],
            subject: "Hello".into(),
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
        address_id: "a".into(),
        size: 0,
        signature: SignatureStatus::Unsigned,
        raw_headers: "Subject: Hello\r\nContent-Type: multipart/mixed;\r\n boundary=\"old\"\r\nMIME-Version: 1.0\r\nX-Keep: \
                      yes\r\n folded\r\nContent-Transfer-Encoding: 7bit\r\n"
            .into(),
        unsubscribe: None,
        receipt_requested: false,
        cc: vec![],
        bcc: vec![],
        reply_to: vec![],
        body: "\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}, <b>\u{43c}\u{438}\u{440}</b>".into(),
        safe_body: String::new(),
        protection: MailProtection::default(),
        details: details_of(""),
        html,
        attachments: vec![],
        from_proton: Some(false),
    }
}

fn file(name: &str, mime: &str, inline: bool, content_id: Option<&str>) -> Attachment {
    Attachment {
        id: String::new(),
        name: name.to_string(),
        size: 0,
        mime: mime.to_string(),
        key_packets: String::new(),
        signed: false,
        inline,
        content_id: content_id.map(str::to_string),
        previewable: false,
    }
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn old_structure_headers_go_and_the_rest_stay() {
    let text = text_of(&build(&message(true), &[]));
    let head = text.split("\r\n\r\n").next().unwrap_or_default();
    assert!(head.contains("Subject: Hello"));
    assert!(head.contains("X-Keep: yes\r\n folded"));
    assert!(!head.contains("multipart"));
    assert!(!head.contains("boundary"));
    assert!(!head.contains("7bit"));
    assert_eq!(head.matches("MIME-Version").count(), 1);
}

#[test]
fn a_mail_without_files_reads_back() {
    let parsed = parse_mime(&build(&message(true), &[]));
    assert_eq!(parsed.html.as_deref(), Some("\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}, <b>\u{43c}\u{438}\u{440}</b>"));
    let parsed = parse_mime(&build(&message(false), &[]));
    assert_eq!(parsed.text.as_deref(), Some("\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}, <b>\u{43c}\u{438}\u{440}</b>"));
}

#[test]
fn files_read_back_whole() {
    let data: Vec<u8> = (0..=255).cycle().take(5000).collect();
    let files = vec![
        (file("\u{43e}\u{442}\u{447}\u{451}\u{442}.pdf", "application/pdf", false, None), data.clone()),
        (file("logo.png", "image/png", true, Some("<logo@mail>")), vec![1, 2, 3]),
    ];
    let parsed = parse_mime(&build(&message(true), &files));
    assert_eq!(parsed.html.as_deref(), Some("\u{41f}\u{440}\u{438}\u{432}\u{435}\u{442}, <b>\u{43c}\u{438}\u{440}</b>"));
    assert_eq!(parsed.attachments.len(), 2);
    assert_eq!(parsed.attachments[0].data, data);
    assert_eq!(parsed.attachments[1].data, [1, 2, 3]);
    assert_eq!(parsed.attachments[1].content_id.as_deref(), Some("logo@mail"));
    assert!(parsed.attachments[1].inline);
}

#[test]
fn hostile_names_cannot_add_headers() {
    let evil = "a\r\nBcc: victim@example.org\r\n\r\nbody";
    let files = vec![(file(evil, "text/plain\r\nX-Evil: 1", false, Some("id\r\nX-Evil: 2")), vec![0])];
    let text = text_of(&build(&message(true), &files));
    assert!(!text.contains("\r\nBcc:"));
    assert!(!text.contains("\r\nX-Evil"));
    assert!(text.contains("Content-Type: application/octet-stream"));
    assert!(!text.contains("Content-ID"));
}

#[test]
fn no_line_is_longer_than_mail_allows() {
    let mut long = message(true);
    long.body = "x".repeat(10_000);
    let bytes = build(&long, &[(file("a", "text/plain", false, None), vec![0; 10_000])]);
    let body = text_of(&bytes);
    let after = body.split("\r\n\r\n").skip(1).collect::<String>();
    assert!(after.lines().all(|line| line.len() <= 998));
}
