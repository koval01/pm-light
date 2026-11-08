//! Translates Proton's replies into the model, and the model's choices into Proton's parameters.

use crate::{
    model::{PaymentMethod, Invoice, InvoiceState, Billing, BillingPlan, Account, Plan, AccountSession, AuthLogEntry, Referrals, Referred, ReferredState, SmtpToken, Organization, Member, MemberState, Habits, UserChoices, MailSettings, UnsubscribeMethods, MailtoUnsubscribe, AutoReply, Folder, Address, MessageMeta, Attachment, ListOptions, Counts, Count},
    safety,
    text::{plaintext_to_html, to_text},
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use url::form_urlencoded;

mod account;
mod protection;
mod settings;
pub use account::*;
pub use protection::*;
pub use settings::*;

const SYSTEM: &[(&str, &str)] = &[
    ("inbox", "0"),
    ("starred", "10"),
    ("drafts", "8"),
    ("scheduled", "12"),
    ("snoozed", "16"),
    ("sent", "7"),
    ("archive", "6"),
    ("spam", "4"),
    ("trash", "3"),
    ("all", "5"),
];

pub const STARRED: &str = "10";
/// Also what has been moved out of Drafts and Sent, and all mail but Spam and Trash.
const ALL_DRAFTS: &str = "1";
const ALL_SENT: &str = "2";
const ALMOST_ALL_MAIL: &str = "15";
pub const LABEL_TYPE_LABEL: u8 = 1;
pub const LABEL_TYPE_FOLDER: u8 = 3;
/// One of the colours Proton's palette accepts; others answer 422.
pub const DEFAULT_COLOR: &str = "#8080FF";
/// `INCOMING_DEFAULTS_LOCATION.BLOCKED`
pub const LOCATION_BLOCKED: u8 = 14;
/// `FILTER_VERSION`: Sieve with the variables extension.
pub const SIEVE_VERSION: u8 = 2;
pub const BATCH_SIZE: usize = 150;

const FLAG_SENT: u64 = 2;
const FLAG_RECEIPT_SENT: u64 = 1 << 11;
const REPEAT_FIXED: u64 = 0;
const REPEAT_PERMANENT: u64 = 4;
const PLAN_TYPE_BASE: i64 = 1;
const MAX_PAGE_SIZE: u64 = 150;
const DEFAULT_PAGE_SIZE: u64 = 50;

/// Sets a field of a JSON object. Indexing a `Value` panics when it is not one, so this does
/// nothing instead: every value given here is an object the code itself just built.
pub fn put(object: &mut Value, key: &str, value: Value) {
    if let Some(fields) = object.as_object_mut() {
        fields.insert(key.to_string(), value);
    }
}

#[must_use]
pub fn label_of(folder: &str) -> &str {
    SYSTEM.iter().find(|(name, _)| *name == folder).map_or(folder, |(_, label)| label)
}

/// The list a folder is read from: the account can widen some of them.
#[must_use]
pub fn listed_label<'a>(folder: &'a str, habits: &Habits) -> &'a str {
    match folder {
        "drafts" if habits.moved_in_drafts => ALL_DRAFTS,
        "sent" if habits.moved_in_sent => ALL_SENT,
        "all" if habits.almost_all_mail => ALMOST_ALL_MAIL,
        _ => label_of(folder),
    }
}

fn folder_of_label(label: &str) -> Option<&'static str> {
    SYSTEM.iter().find(|(_, id)| *id == label).map(|(name, _)| *name)
}

#[must_use]
pub fn system_folders() -> Vec<Folder> {
    SYSTEM
        .iter()
        .map(|(name, _)| Folder { id: (*name).into(), name: (*name).into(), kind: "system", color: None })
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ApiAddr {
    pub name: Option<String>,
    pub address: String,
    /// Set by Proton for its own mail, whoever the address claims to be.
    #[serde(default)]
    pub is_proton: u8,
}

impl From<&ApiAddr> for Address {
    fn from(a: &ApiAddr) -> Self {
        Self { name: a.name.clone().filter(|n| !n.is_empty()), email: a.address.clone() }
    }
}

fn addresses(list: &[ApiAddr]) -> Vec<Address> {
    list.iter().map(Address::from).collect()
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiMessageMeta {
    #[serde(rename = "ID")]
    pub id: String,
    pub subject: String,
    pub sender: ApiAddr,
    pub to_list: Vec<ApiAddr>,
    #[serde(rename = "CCList")]
    pub cc_list: Vec<ApiAddr>,
    #[serde(rename = "BCCList")]
    pub bcc_list: Vec<ApiAddr>,
    pub time: u64,
    pub unread: u8,
    #[serde(rename = "LabelIDs")]
    pub label_ids: Vec<String>,
    #[serde(rename = "AddressID")]
    pub address_id: String,
    pub num_attachments: u64,
    #[serde(rename = "ConversationID")]
    pub conversation_id: String,
    pub flags: u64,
    /// When Proton deletes the message by itself, in seconds: from the trash or spam after their days, or as its
    /// sender set it. 0 when it stays.
    pub expiration_time: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiAttachment {
    #[serde(rename = "ID")]
    pub id: String,
    pub name: String,
    pub size: u64,
    #[serde(rename = "MIMEType")]
    pub mime_type: String,
    pub key_packets: String,
    pub signature: Option<String>,
    pub disposition: Option<String>,
    pub headers: HashMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiMailto {
    pub to_list: Vec<String>,
    pub subject: Option<String>,
    pub body: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiUnsubscribe {
    pub one_click: Option<String>,
    pub http_client: Option<String>,
    pub mailto: Option<ApiMailto>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiMessage {
    #[serde(flatten)]
    pub meta: ApiMessageMeta,
    pub body: String,
    pub header: Option<String>,
    pub size: u64,
    pub parsed_headers: Map<String, Value>,
    pub unsubscribe_methods: Option<ApiUnsubscribe>,
    #[serde(rename = "MIMEType")]
    pub mime_type: String,
    pub reply_tos: Vec<ApiAddr>,
    pub attachments: Vec<ApiAttachment>,
}

pub fn to_meta(m: &ApiMessageMeta, current_folder: Option<&str>) -> MessageMeta {
    let real = |folder: &&str| *folder != "starred" && *folder != "all";
    let folder = current_folder
        .filter(real)
        .map(str::to_string)
        .or_else(|| m.label_ids.iter().filter_map(|l| folder_of_label(l)).find(real).map(str::to_string))
        .unwrap_or_else(|| "inbox".to_string());
    MessageMeta {
        id: m.id.clone(),
        folder,
        from: (&m.sender).into(),
        to: addresses(&m.to_list),
        subject: m.subject.clone(),
        snippet: String::new(), // Proton encrypts previews; they are not available in metadata
        date: m.time * 1000,
        unread: m.unread == 1,
        starred: m.label_ids.iter().any(|l| l == STARRED),
        labels: m.label_ids.clone(),
        attachment_count: m.num_attachments,
        conversation_id: m.conversation_id.clone(),
        messages: 0,
        expires: m.expiration_time,
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiConversationLabel {
    #[serde(rename = "ID")]
    pub id: String,
    pub context_time: u64,
    pub context_num_unread: u32,
    pub context_num_attachments: u64,
    /// The same for the messages of the conversation in this folder.
    pub context_expiration_time: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct ApiConversation {
    #[serde(rename = "ID")]
    pub id: String,
    pub subject: String,
    pub senders: Vec<ApiAddr>,
    pub recipients: Vec<ApiAddr>,
    pub time: u64,
    pub num_messages: u32,
    pub num_unread: u32,
    pub num_attachments: u64,
    pub labels: Vec<ApiConversationLabel>,
}

/// Everyone who wrote in it, under the address of the first, which is what a logo is found by.
fn senders_of(c: &ApiConversation) -> Address {
    let Some(first) = c.senders.first() else { return Address { name: None, email: String::new() } };
    let mut names: Vec<&str> = Vec::new();
    for sender in &c.senders {
        let name = sender.name.as_deref().filter(|name| !name.is_empty()).unwrap_or(&sender.address);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if names.len() < 2 { first.into() } else { Address { name: Some(names.join(", ")), email: first.address.clone() } }
}

/// A conversation as a row of a folder: what counts is what it holds in that folder, and it goes
/// by the time of the newest mail there. The conversation's own id stands where a message's would.
#[must_use]
pub fn conversation_row(c: &ApiConversation, folder: &str) -> MessageMeta {
    let label = label_of(folder);
    let here = c.labels.iter().find(|known| known.id == label);
    let context = |value: fn(&ApiConversationLabel) -> u64| here.map_or(0, value);
    let time = Some(context(|here| here.context_time)).filter(|time| *time > 0).unwrap_or(c.time);
    let unread = here.map_or(c.num_unread > 0, |here| here.context_num_unread > 0);
    let files = Some(context(|here| here.context_num_attachments)).filter(|files| *files > 0);
    MessageMeta {
        id: c.id.clone(),
        folder: folder.to_string(),
        from: senders_of(c),
        to: addresses(&c.recipients),
        subject: c.subject.clone(),
        snippet: String::new(),
        date: time * 1000,
        unread,
        starred: c.labels.iter().any(|known| known.id == STARRED),
        labels: c.labels.iter().map(|known| known.id.clone()).collect(),
        attachment_count: files.unwrap_or(c.num_attachments),
        conversation_id: c.id.clone(),
        messages: c.num_messages.max(1),
        expires: here.map_or(0, |here| here.context_expiration_time),
    }
}

fn is_draft(row: &MessageMeta) -> bool {
    row.labels.iter().any(|label| label == ALL_DRAFTS)
}

/// From the end, the earliest of the unread run that closes the list. The first one if all are.
fn first_to_read(rows: &[&MessageMeta]) -> Option<String> {
    let after_read = rows.iter().rposition(|row| !row.unread).map_or(0, |at| at + 1);
    rows.get(after_read).or_else(|| rows.last()).map(|row| row.id.clone())
}

/// Which mail of a conversation opens, as `findMessageToExpand` in Proton's client has it: in a
/// label only what wears it, and otherwise the newest if it has been read, or the first not yet read.
#[must_use]
pub fn message_to_open(thread: &[MessageMeta], folder: &str) -> Option<String> {
    let mut rows: Vec<&MessageMeta> = thread.iter().collect();
    rows.sort_by_key(|row| row.date);
    let label = label_of(folder);
    let chosen = |keep: &dyn Fn(&MessageMeta) -> bool| rows.iter().copied().filter(|row| keep(row)).collect::<Vec<_>>();
    let is_system_folder = SYSTEM.iter().any(|(name, _)| *name == folder) && folder != "starred";
    let wanted = if is_system_folder {
        chosen(&|row| !is_draft(row))
    } else {
        chosen(&|row| !is_draft(row) && row.labels.iter().any(|known| known == label))
    };
    let opened = match wanted.last() {
        Some(latest) if is_system_folder && !latest.unread => Some(latest.id.clone()),
        _ => first_to_read(&wanted),
    };
    opened.or_else(|| rows.last().map(|row| row.id.clone()))
}

#[must_use]
pub fn cc_of(m: &ApiMessageMeta) -> Vec<Address> {
    addresses(&m.cc_list)
}

#[must_use]
pub fn bcc_of(m: &ApiMessageMeta) -> Vec<Address> {
    addresses(&m.bcc_list)
}

#[must_use]
pub fn reply_to_of(m: &ApiMessage) -> Vec<Address> {
    addresses(&m.reply_tos)
}

#[must_use]
pub fn to_attachment(a: &ApiAttachment) -> Attachment {
    let header = |name: &str| {
        a.headers.get(name).and_then(|value| match value {
            Value::Array(list) => list.first().and_then(Value::as_str),
            other => other.as_str(),
        })
    };
    let content_id = header("content-id")
        .map(|id| id.trim().trim_start_matches('<').trim_end_matches('>').to_string())
        .filter(|id| !id.is_empty());
    let disposition = a.disposition.as_deref().or_else(|| header("content-disposition")).unwrap_or_default();
    Attachment {
        previewable: safety::can_preview(&a.mime_type),
        id: a.id.clone(),
        name: if a.name.is_empty() { a.id.clone() } else { a.name.clone() },
        size: a.size,
        mime: if a.mime_type.is_empty() { "application/octet-stream".into() } else { a.mime_type.clone() },
        key_packets: a.key_packets.clone(),
        signed: a.signature.as_deref().is_some_and(|s| !s.is_empty()),
        inline: disposition.to_lowercase().starts_with("inline"),
        content_id,
    }
}

#[must_use]
pub fn list_query(folder: &str, options: &ListOptions) -> String {
    let limit = options.limit.unwrap_or(DEFAULT_PAGE_SIZE).max(1);
    let page = options.offset.unwrap_or(0) / limit;
    let mut query = form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("LabelID", label_of(folder))
        .append_pair("Page", &page.to_string())
        .append_pair("PageSize", &limit.min(MAX_PAGE_SIZE).to_string())
        .append_pair("Sort", "Time")
        .append_pair("Desc", "1");
    if let Some(keyword) = options.keyword.as_deref().filter(|k| !k.is_empty()) {
        query.append_pair("Keyword", keyword);
    }
    if let Some(unread) = options.unread {
        query.append_pair("Unread", if unread { "1" } else { "0" });
    }
    if let Some(starred) = options.starred {
        query.append_pair("Starred", if starred { "1" } else { "0" });
    }
    if options.has_attachments == Some(true) {
        query.append_pair("Attachments", "1");
    }
    for (name, value) in [("From", &options.from), ("Recipients", &options.to), ("Subject", &options.subject)] {
        if let Some(value) = value.as_deref().filter(|value| !value.is_empty()) {
            query.append_pair(name, value);
        }
    }
    for (name, value) in [("Begin", options.begin), ("End", options.end)] {
        if let Some(value) = value {
            query.append_pair(name, &value.to_string());
        }
    }
    query.finish()
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ApiCount {
    #[serde(rename = "LabelID")]
    pub label_id: String,
    pub total: u64,
    pub unread: u64,
}

#[must_use]
pub fn to_counts(rows: &[ApiCount]) -> Counts {
    rows.iter()
        .map(|row| {
            let folder = folder_of_label(&row.label_id).map_or_else(|| row.label_id.clone(), str::to_string);
            (folder, Count { total: row.total, unread: row.unread })
        })
        .collect()
}

/// Only growth counts: reading mail on another device lowers the number.
#[must_use]
pub fn new_mail_count(before: &Counts, after: &Counts) -> u64 {
    let unread = |counts: &Counts| counts.get("inbox").map_or(0, |c| c.unread);
    unread(after).saturating_sub(unread(before))
}

#[must_use]
pub fn needs_read_receipt(flags: u64, headers: &Map<String, Value>) -> bool {
    headers.contains_key("Disposition-Notification-To")
        && flags & FLAG_SENT == 0
        && flags & FLAG_RECEIPT_SENT == 0
}

#[cfg(test)]
mod tests;
