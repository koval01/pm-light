//! Everything about a message being written, except the typing. The interface holds the state, and
//! Rust fills it in, checks it and turns it into what Proton receives.

use crate::{
    error::{Error, Result},
    html::for_editor,
    model::{Address, Attachment, ContactEmail, Habits, Message, Outgoing, OutgoingFile, OwnAddress},
    text::{escape, plaintext_to_html},
};
use lol_html::{RewriteStrSettings, element, html_content::ContentType, rewrite_str};
use crate::pattern::Pattern;
use serde::{Deserialize, Serialize};
use std::cell::Cell;

const EMPTY_LINE: &str = "<div><br></div>";
const NO_SUBJECT: &str = "(No Subject)";
const FOOTER_LINK: &str = "https://proton.me/mail/home";
const SIGNATURE_CLASS: &str = "protonmail_signature_block";
const ACTION_REPLY: u8 = 0;
const ACTION_REPLY_ALL: u8 = 1;
const ACTION_FORWARD: u8 = 2;

pub(crate) static BRACKETED: Pattern = Pattern::new(r"<([^>]+)>");
// ASCII only: a Cyrillic letter in an address is a typing mistake far more often than it is real, and
// an international domain has an ASCII form that starts with `xn--`.
pub(crate) static VALID_ADDRESS: Pattern = Pattern::new(
    r"^[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@([A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?\.)+[A-Za-z][A-Za-z0-9-]*[A-Za-z0-9]$",
);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ComposeMode {
    New,
    Reply,
    ReplyAll,
    Forward,
    Draft,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeState {
    pub mode: ComposeMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<u8>,
    pub existing: Vec<Attachment>,
    pub files: Vec<OutgoingFile>,
    /// The body is text and goes out as `text/plain`, and not HTML from the editor.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub plain: bool,
}

impl ComposeState {
    const fn empty(mode: ComposeMode) -> Self {
        Self {
            mode,
            draft_id: None,
            from: None,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            subject: String::new(),
            body: String::new(),
            parent_id: None,
            action: None,
            existing: Vec::new(),
            files: Vec::new(),
            plain: false,
        }
    }
}

/// What goes into a message in the reader's language. The date is already formatted for their locale.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComposeLabels {
    pub date_time: String,
    pub forwarded: String,
    pub from: String,
    pub date: String,
    pub subject: String,
    pub to: String,
    /// Proton's line under a letter, with `{link}` where its name goes. Nothing leaves it out.
    #[serde(default)]
    pub proton_footer: String,
}

/// Choices made when sending, beyond the message itself.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendOptions {
    pub deliver_at: Option<u64>,
    pub auto_save_contacts: Option<bool>,
    pub delay_seconds: Option<u64>,
    pub password: Option<String>,
    pub password_hint: Option<String>,
    pub expires_in: Option<u64>,
    pub read_receipt: Option<bool>,
}

/// Accepts "a@b.c, Name <d@e.f>; g@h.i" and returns bare addresses, without duplicates.
pub fn parse_addresses(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in input.split([',', ';', '\n']) {
        let email = BRACKETED.captures(part).map_or(part, |c| c.get(1).map_or(part, |m| m.as_str())).trim();
        if !email.is_empty() && !out.iter().any(|known| known.eq_ignore_ascii_case(email)) {
            out.push(email.to_string());
        }
    }
    out
}

/// Addresses as typed or pasted. Here a space separates too: no address has one.
#[must_use]
pub fn typed_recipients(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in parse_addresses(input).iter().flat_map(|address| address.split_whitespace()) {
        if !out.iter().any(|known| known.eq_ignore_ascii_case(part)) {
            out.push(part.to_string());
        }
    }
    out
}

/// Contacts for what is being typed: those whose address starts with it first, then any other match.
#[must_use]
pub fn suggestions(contacts: &[ContactEmail], typed: &str, taken: &[String], limit: usize) -> Vec<ContactEmail> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return Vec::new();
    }
    let free = contacts.iter().filter(|c| !taken.iter().any(|known| known.eq_ignore_ascii_case(&c.email)));
    let (mut first, mut rest) = (Vec::new(), Vec::new());
    for contact in free {
        let email = contact.email.to_lowercase();
        if email.starts_with(&typed) {
            first.push(contact.clone());
        } else if email.contains(&typed) || contact.name.to_lowercase().contains(&typed) {
            rest.push(contact.clone());
        }
    }
    first.extend(rest);
    first.truncate(limit);
    first
}

pub fn is_valid_address(address: &str) -> bool {
    VALID_ADDRESS.is_match(address)
}

/// The domain of a valid address, in the form it is compared in.
#[must_use]
pub fn domain_of(address: &str) -> Option<String> {
    let (_, domain) = address.rsplit_once('@').filter(|_| is_valid_address(address))?;
    Some(domain.to_ascii_lowercase())
}

/// Proton's own prefixes, plus "Fwd:" so that other clients' forwards do not stack.
fn prefixed(prefix: &str, subject: &str) -> String {
    let lower = subject.trim().to_lowercase();
    let known = if prefix == "Fw" { lower.starts_with("fw:") || lower.starts_with("fwd:") } else { lower.starts_with("re:") };
    if known { subject.to_string() } else { format!("{prefix}: {subject}") }
}

/// Proton answers with this for a draft saved with no subject, and it must not turn into a real one.
fn own_subject(subject: &str) -> String {
    if subject == NO_SUBJECT { String::new() } else { subject.to_string() }
}

fn as_html(m: &Message) -> String {
    if m.html { for_editor(&m.safe_body) } else { plaintext_to_html(&m.body) }
}

fn sender_line(m: &Message) -> String {
    let from = &m.meta.from;
    escape(&from.name.as_ref().map_or_else(|| from.email.clone(), |name| format!("{name} <{}>", from.email)))
}

fn emails(addresses: &[Address]) -> Vec<String> {
    addresses.iter().map(|a| a.email.clone()).collect()
}

fn quote(m: &Message, labels: &ComposeLabels) -> String {
    let line = format!("{}, {}:", escape(&labels.date_time), sender_line(m));
    format!(r#"{EMPTY_LINE}<div class="protonmail_quote">{line}<blockquote>{}</blockquote></div>"#, as_html(m))
}

fn forwarded(m: &Message, labels: &ComposeLabels) -> String {
    let header = [
        format!("---------- {} ----------", escape(&labels.forwarded)),
        format!("{}: {}", escape(&labels.from), sender_line(m)),
        format!("{}: {}", escape(&labels.date), escape(&labels.date_time)),
        format!("{}: {}", escape(&labels.subject), escape(&m.meta.subject)),
        format!("{}: {}", escape(&labels.to), escape(&emails(&m.meta.to).join(", "))),
    ]
    .join("<br>");
    format!(r#"{EMPTY_LINE}<div class="protonmail_quote">{header}<br><br>{}</div>"#, as_html(m))
}

#[must_use]
pub fn signature_for<'a>(own: &'a [OwnAddress], email: Option<&str>) -> &'a str {
    own.iter()
        .find(|a| Some(a.email.as_str()) == email)
        .or_else(|| own.first())
        .map_or("", |a| a.signature.as_str())
}

/// Under the address's own signature with a line between, as Proton's client writes it. The words
/// come from the app's dictionary and the link is fixed, so nothing here comes from outside.
#[must_use]
pub fn with_footer(signature: &str, words: &str) -> String {
    let link = format!(r#"<a href="{FOOTER_LINK}" target="_blank">Proton Mail</a>"#);
    let line = format!("<div>{}</div>", escape(words).replace("{link}", &link));
    if signature.is_empty() { line } else { format!("{signature}{EMPTY_LINE}{line}") }
}

fn signature_block(signature: &str) -> String {
    if signature.is_empty() {
        String::new()
    } else {
        format!(r#"{EMPTY_LINE}<div class="{SIGNATURE_CLASS}">{signature}</div>"#)
    }
}

/// Puts the new address's signature in place of the old one when the sender changes.
#[must_use]
pub fn replace_signature(body: &str, signature: &str) -> String {
    let found = Cell::new(false);
    let selector = format!(".{SIGNATURE_CLASS}");
    let rewritten = rewrite_str(
        body,
        RewriteStrSettings {
            element_content_handlers: vec![element!(selector, |el| {
                found.set(true);
                if signature.is_empty() {
                    el.remove();
                } else {
                    el.set_inner_content(signature, ContentType::Html);
                }
                Ok(())
            })],
            ..RewriteStrSettings::new()
        },
    )
    .unwrap_or_else(|_| body.to_string());
    if !found.get() && !signature.is_empty() {
        return format!("{EMPTY_LINE}{}{body}", signature_block(signature));
    }
    rewritten
}

/// A new message, or one that starts from `message`.
#[must_use]
pub fn compose_from(
    mode: ComposeMode,
    message: Option<&Message>,
    own: &[OwnAddress],
    labels: &ComposeLabels,
) -> ComposeState {
    let empty = ComposeState::empty(mode);
    let Some(m) = message.filter(|_| mode != ComposeMode::New) else {
        return ComposeState { body: format!("{EMPTY_LINE}{}", signature_block(signature_for(own, None))), ..empty };
    };
    // Answer from the address the mail came to, so that an alias stays an alias.
    let from = own.iter().find(|a| a.id == m.address_id).map(|a| a.email.clone());
    let signature = signature_block(signature_for(own, from.as_deref()));

    match mode {
        ComposeMode::Draft => ComposeState {
            from,
            draft_id: Some(m.meta.id.clone()),
            to: emails(&m.meta.to).join(", "),
            cc: emails(&m.cc).join(", "),
            bcc: emails(&m.bcc).join(", "),
            subject: own_subject(&m.meta.subject),
            body: as_html(m),
            existing: m.attachments.clone(),
            ..empty
        },
        ComposeMode::Forward => ComposeState {
            from,
            subject: prefixed("Fw", &m.meta.subject),
            body: format!("{EMPTY_LINE}{signature}{}", forwarded(m, labels)),
            parent_id: Some(m.meta.id.clone()),
            action: Some(ACTION_FORWARD),
            ..empty
        },
        _ => {
            let mine = |email: &str| own.iter().any(|a| a.email.eq_ignore_ascii_case(email));
            let reply_to = if m.reply_to.is_empty() { vec![m.meta.from.email.clone()] } else { emails(&m.reply_to) };
            // Replying to our own sent mail continues to the original recipients.
            let to = if mine(&m.meta.from.email) { emails(&m.meta.to) } else { reply_to };
            let everyone = m.meta.to.iter().chain(&m.cc).map(|a| a.email.clone());
            let cc: Vec<String> = if mode == ComposeMode::ReplyAll {
                everyone.filter(|email| !mine(email) && !to.contains(email)).collect()
            } else {
                Vec::new()
            };
            ComposeState {
                from,
                to: to.join(", "),
                cc: cc.join(", "),
                subject: prefixed("Re", &m.meta.subject),
                body: format!("{EMPTY_LINE}{signature}{}", quote(m, labels)),
                parent_id: Some(m.meta.id.clone()),
                action: Some(if mode == ComposeMode::ReplyAll { ACTION_REPLY_ALL } else { ACTION_REPLY }),
                ..empty
            }
        }
    }
}

/// The line a new letter starts on, in the font and direction the account asks for. Writing goes on
/// from there, so the rest of the letter follows it. The font comes from a closed table.
#[must_use]
pub fn styled_first_line(body: &str, habits: &Habits) -> String {
    let Some(rest) = body.strip_prefix(EMPTY_LINE) else { return body.to_string() };
    let face = habits.font_face.as_ref().map(|face| format!("font-family: {face};"));
    let size = habits.font_size.map(|size| format!("font-size: {size}px;"));
    let style = [face, size].into_iter().flatten().collect::<Vec<_>>().join(" ");
    let style = if style.is_empty() { String::new() } else { format!(r#" style="{style}""#) };
    let direction = if habits.right_to_left { r#" dir="rtl""# } else { "" };
    if style.is_empty() && direction.is_empty() {
        return body.to_string();
    }
    format!("<div{style}{direction}><br></div>{rest}")
}

/// A message prepared by a `mailto:` link or a newsletter's "unsubscribe by mail".
#[must_use]
pub fn compose_prepared(to: &[String], subject: &str, body: &str) -> ComposeState {
    ComposeState {
        to: to.join(", "),
        subject: subject.to_string(),
        body: format!("<div>{}</div>", escape(&body.replace("\r\n", "\n")).replace('\n', "<br>")),
        ..ComposeState::empty(ComposeMode::New)
    }
}

fn outgoing(state: &ComposeState, to: Vec<String>, cc: Vec<String>, bcc: Vec<String>, options: SendOptions) -> Outgoing {
    Outgoing {
        draft_id: state.draft_id.clone(),
        from: state.from.clone(),
        to,
        cc,
        bcc,
        subject: state.subject.clone(),
        body: state.body.clone(),
        mime: if state.plain { "text/plain" } else { "text/html" }.to_string(),
        files: state.files.clone(),
        existing: state.existing.clone(),
        parent_id: state.parent_id.clone(),
        action: state.action,
        deliver_at: options.deliver_at,
        auto_save_contacts: options.auto_save_contacts,
        delay_seconds: options.delay_seconds,
        password: options.password.filter(|p| !p.is_empty()),
        password_hint: options.password_hint,
        expires_in: options.expires_in,
        read_receipt: options.read_receipt,
    }
}

/// What goes out. An address that does not look real stops it, by name.
pub fn to_outgoing(state: &ComposeState, options: SendOptions) -> Result<Outgoing> {
    let (to, cc, bcc) = (parse_addresses(&state.to), parse_addresses(&state.cc), parse_addresses(&state.bcc));
    if let Some(invalid) = to.iter().chain(&cc).chain(&bcc).find(|a| !is_valid_address(a)) {
        return Err(Error::InvalidAddress(invalid.clone()));
    }
    Ok(outgoing(state, to, cc, bcc, options))
}

/// Autosaving must not fail on an address the person is still typing.
#[must_use]
pub fn draft_outgoing(state: &ComposeState) -> Outgoing {
    let valid = |field: &str| parse_addresses(field).into_iter().filter(|a| is_valid_address(a)).collect();
    outgoing(state, valid(&state.to), valid(&state.cc), valid(&state.bcc), SendOptions::default())
}

pub mod plain;

#[cfg(test)]
mod tests;
