//! A made-up mail, laid out as a real one is: for checking, on a system no account is signed in to, how a picture of
//! a mail and a printed sheet come out of its web engine. Nothing in it is anyone's.

use super::{PrintDocument, PrintItem, PrintLabels, PrintOptions, document};
use std::fmt::Write as _;
use crate::{
    crypto::SignatureStatus,
    details::MessageDetails,
    model::{Address, MailProtection, Message, MessageMeta},
};

/// 2026-01-15 10:30 UTC, in milliseconds, as a message's date is.
const SENT: u64 = 1_768_473_000_000;
/// A blue square, as a picture written into the mail: one that needs nothing from outside.
const SQUARE: &str = "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI2NCIgaGVpZ2h0PSI2NCI+PHJlY3Qgd2lkdGg9IjY0IiBoZWlnaHQ9IjY0IiBmaWxsPSIjMmY2MmRjIi8+PC9zdmc+";

fn body() -> String {
    let mut rows = String::new();
    for row in 1..=4 {
        let _ = write!(rows, "<tr><td>Item {row}</td><td style=\"text-align:right\">{}.00</td></tr>", row * 12);
    }
    let mut lines = String::new();
    for line in 1..=12 {
        let _ = write!(lines, "<li>Line {line} of a list long enough to need more than one screen.</li>");
    }
    format!(
        "<h2 style=\"color:#2f62dc\">A sample mail</h2>\
         <p>This mail exists only to check how a mail is drawn. Its text has <b>bold</b>, <i>italic</i> and \
         <a href=\"https://example.com\">a link</a>.</p>\
         <p><img src=\"{SQUARE}\" width=\"64\" height=\"64\" alt=\"A blue square\"></p>\
         <table style=\"border-collapse:collapse\" border=\"1\" cellpadding=\"6\">{rows}</table>\
         <ol>{lines}</ol>\
         <p style=\"background:#fff3c4;padding:8px\">The end of the sample.</p>"
    )
}

fn message() -> Message {
    let body = body();
    Message {
        meta: MessageMeta {
            id: "sample".into(),
            folder: "inbox".into(),
            from: Address { name: Some("Sample Sender".into()), email: "sender@example.com".into() },
            to: vec![Address { name: None, email: "reader@example.com".into() }],
            subject: "A sample mail".into(),
            snippet: String::new(),
            date: SENT,
            unread: false,
            starred: false,
            labels: Vec::new(),
            attachment_count: 0,
            conversation_id: "sample".into(),
            messages: 0,
            expires: 0,
        },
        address_id: String::new(),
        size: u64::try_from(body.len()).unwrap_or(0),
        signature: SignatureStatus::Unsigned,
        raw_headers: String::new(),
        unsubscribe: None,
        receipt_requested: false,
        cc: Vec::new(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        body: body.clone(),
        safe_body: body,
        protection: MailProtection::default(),
        details: MessageDetails { encryption: "end-to-end".into(), auth: Vec::new(), message_id: "<sample@example.com>".into() },
        html: true,
        attachments: Vec::new(),
        from_proton: Some(false),
    }
}

/// The made-up mail as a document, as `print_document` makes one of a real mail.
#[must_use]
pub fn sample_document(options: &PrintOptions, labels: &PrintLabels) -> PrintDocument {
    let message = message();
    let item = PrintItem { message: &message, body: message.safe_body.clone(), earlier: None, seal: None };
    document(&[item], options, labels)
}

#[cfg(test)]
mod tests;
