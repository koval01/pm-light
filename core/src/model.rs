//! What the interface receives and sends. Field names match its TypeScript types.

use crate::{crypto::SignatureStatus, details::MessageDetails};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;


mod protection;
pub use protection::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Count {
    pub total: u64,
    pub unread: u64,
}

pub type Counts = BTreeMap<String, Count>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Address {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub email: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    pub key_packets: String,
    pub signed: bool,
    pub inline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    /// Shown inside the app instead of being saved.
    #[serde(default)]
    pub previewable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSource {
    pub headers: String,
    pub body: String,
}

/// Kept in the account, in Proton's numbering: 0 always means "as the language has it".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSettings {
    pub time_format: u8,
    pub date_format: u8,
    pub week_start: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TimeSetting {
    TimeFormat,
    DateFormat,
    WeekStart,
}

impl Message {
    /// What the reader draws. The rest is large and rarely wanted, and every byte sent to the page
    /// stays in its heap for a while: the source and headers are asked for when someone opens them.
    #[must_use]
    pub fn for_reader(&self) -> Self {
        let mut message = self.clone();
        message.raw_headers = String::new();
        if message.html {
            message.body = String::new();
        }
        message
    }
}

/// What an account says about itself besides mail.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(clippy::struct_excessive_bools, reason = "each is one of Proton's own switches, sent and read as such")]
pub struct UserChoices {
    pub telemetry: bool,
    pub crash_reports: bool,
    pub news: u64,
    pub breach_alerts_offered: bool,
    pub breach_alerts: bool,
    pub sentinel_offered: bool,
    pub sentinel: bool,
    pub invoice_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReferredState {
    Invited,
    SignedUp,
    Trial,
    Completed,
    Rewarded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Referred {
    /// Only for those invited by mail.
    pub email: String,
    pub created_at: u64,
    pub state: ReferredState,
}

/// Amounts are in cents of `currency`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Referrals {
    pub link: String,
    pub reward_months: u64,
    pub reward_limit: u64,
    pub emails_left: u64,
    pub currency: String,
    pub for_you: u64,
    pub for_friend: u64,
    pub most: u64,
    pub people: Vec<Referred>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmtpToken {
    pub id: String,
    pub name: String,
    pub address_id: String,
    pub created_at: u64,
    /// Zero if it was never used.
    pub last_used: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MemberState {
    Disabled,
    Enabled,
    Invited,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub name: String,
    pub admin: bool,
    pub own: bool,
    pub state: MemberState,
    pub used_space: u64,
    pub max_space: u64,
}

/// What is used and what the plan allows, as pairs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub name: String,
    pub members: (u64, u64),
    pub addresses: (u64, u64),
    pub domains: (u64, u64),
    pub space: (u64, u64),
    pub people: Vec<Member>,
}

/// Milliseconds to an answer, or nothing if there was none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Latency {
    pub proton: Option<u32>,
    pub quad9: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageMeta {
    pub id: String,
    pub folder: String,
    pub from: Address,
    pub to: Vec<Address>,
    pub subject: String,
    pub snippet: String,
    pub date: u64,
    pub unread: bool,
    pub starred: bool,
    pub labels: Vec<String>,
    pub attachment_count: u64,
    pub conversation_id: String,
    /// How many the row stands for when it is a conversation, and 0 when it is one message.
    #[serde(default)]
    pub messages: u32,
    /// When Proton deletes it by itself, in seconds, and 0 when it stays.
    #[serde(default)]
    pub expires: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct MailtoUnsubscribe {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsubscribeMethods {
    pub one_click: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mailto: Option<MailtoUnsubscribe>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtectedImage {
    pub url: String,
    pub pixel: bool,
    pub tracker: String,
    /// Watching the reader, not merely held back.
    pub threat: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct CleanedLink {
    pub from: String,
    pub to: String,
}

/// What was taken out of a message, or held back, before showing it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct MailProtection {
    pub images: Vec<ProtectedImage>,
    pub links: Vec<CleanedLink>,
    pub removed: Vec<String>,
    /// Everything that protected the reader.
    pub threats: usize,
}

impl MailProtection {
    /// Call last, once trackers are named: a plain remote image is held back, but is no threat.
    pub fn settle(&mut self) {
        for image in &mut self.images {
            image.threat = image.pixel || !image.tracker.is_empty();
        }
        self.threats =
            self.images.iter().filter(|image| image.threat).count() + self.links.len() + self.removed.len();
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    #[serde(flatten)]
    pub meta: MessageMeta,
    pub address_id: String,
    pub size: u64,
    pub signature: SignatureStatus,
    pub raw_headers: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unsubscribe: Option<UnsubscribeMethods>,
    pub receipt_requested: bool,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    /// The decrypted body exactly as it arrived: never rendered, only shown as source and quoted.
    pub body: String,
    /// `body` after sanitizing, for the sandboxed frame.
    pub safe_body: String,
    pub protection: MailProtection,
    pub details: MessageDetails,
    pub html: bool,
    pub attachments: Vec<Attachment>,
    /// Proton itself says this came from Proton. Nothing, for mail kept on disk before this was asked.
    #[serde(default)]
    pub from_proton: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListOptions {
    pub offset: Option<u64>,
    pub limit: Option<u64>,
    pub keyword: Option<String>,
    pub unread: Option<bool>,
    pub has_attachments: Option<bool>,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub begin: Option<u64>,
    #[serde(default)]
    pub end: Option<u64>,
    #[serde(default)]
    pub starred: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageChanges {
    pub unread: Option<bool>,
    pub starred: Option<bool>,
    pub folder: Option<String>,
}

/// A file handed over with `stage_file`, so its bytes need not travel as JSON.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingFile {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub staged_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outgoing {
    pub draft_id: Option<String>,
    pub from: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub mime: String,
    pub files: Vec<OutgoingFile>,
    pub existing: Vec<Attachment>,
    pub parent_id: Option<String>,
    pub action: Option<u8>,
    pub deliver_at: Option<u64>,
    pub auto_save_contacts: Option<bool>,
    pub delay_seconds: Option<u64>,
    pub password: Option<String>,
    pub password_hint: Option<String>,
    pub expires_in: Option<u64>,
    pub read_receipt: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnAddress {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub signature: String,
    /// The signature as plain text, which is how it is edited.
    pub signature_text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[expect(clippy::struct_excessive_bools, reason = "each is one of Proton's own switches, sent and read as such")]
pub struct MailSettings {
    pub delay_send_seconds: u64,
    pub hide_remote_images: bool,
    pub next_message_on_move: bool,
    pub shortcuts: bool,
    pub auto_save_contacts: bool,
    pub confirm_links: bool,
    pub auto_delete_old: bool,
}

/// The account's mail settings this client acts on, known without asking Proton each time.
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "each is one of the account's own switches, as Proton keeps them")]
pub struct Habits {
    pub page_size: u64,
    pub moved_in_drafts: bool,
    pub moved_in_sent: bool,
    pub almost_all_mail: bool,
    pub folder_colors: bool,
    pub wide_composer: bool,
    pub plain_drafts: bool,
    pub right_to_left: bool,
    pub font_face: Option<String>,
    pub font_size: Option<i64>,
    pub skip_block_question: bool,
    pub unsubscribe_on_spam: bool,
    pub conversations: bool,
    pub hide_embedded: bool,
    pub remove_metadata: bool,
    pub proton_footer: bool,
    pub proton_footer_locked: bool,
}

impl Default for Habits {
    fn default() -> Self {
        Self {
            page_size: crate::view::PAGE,
            moved_in_drafts: false,
            moved_in_sent: false,
            almost_all_mail: false,
            folder_colors: false,
            wide_composer: false,
            plain_drafts: false,
            right_to_left: false,
            font_face: None,
            font_size: None,
            skip_block_question: false,
            unsubscribe_on_spam: false,
            conversations: false,
            hide_embedded: false,
            remove_metadata: false,
            proton_footer: false,
            proton_footer_locked: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub title: String,
    pub renews: bool,
    pub period_end: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingPlan {
    pub title: String,
    pub months: u64,
    /// Money is in cents, as Proton sends it.
    pub amount: i64,
    pub renew_amount: i64,
    pub currency: String,
    pub renews: bool,
    pub period_end: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentMethod {
    pub label: String,
    pub is_default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InvoiceState {
    Paid,
    Unpaid,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    pub id: String,
    pub state: InvoiceState,
    pub amount: i64,
    pub currency: String,
    pub created_at: u64,
}

/// What Proton knows about paying for the account. Read only: nothing here changes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Billing {
    pub plan: Option<BillingPlan>,
    pub credit: i64,
    pub currency: String,
    pub methods: Vec<PaymentMethod>,
    pub invoices: Vec<Invoice>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub name: String,
    pub email: String,
    pub created_at: u64,
    pub paid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
    pub two_factor: &'static str,
    pub recovery_email: String,
    pub log_mode: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSession {
    pub id: String,
    pub client: String,
    pub created_at: u64,
    pub revocable: bool,
    pub current: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AuthLogEntry {
    pub time: u64,
    pub event: i64,
    pub description: String,
    pub ip: String,
    pub location: String,
    pub device: String,
    pub failed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoReply {
    pub enabled: bool,
    pub permanent: bool,
    pub start: u64,
    pub end: u64,
    pub subject: String,
    /// Plain text, which is how it is edited. Proton stores HTML.
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MailFilter {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub sieve: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FilterDraft {
    pub id: Option<String>,
    pub enabled: Option<bool>,
    pub name: String,
    pub sieve: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactEmail {
    pub name: String,
    pub email: String,
    pub contact_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Storage {
    pub used: u64,
    pub max: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SavedDraft {
    pub id: String,
    pub attachments: Vec<Attachment>,
}

/// What changed since the last check, for the interface and the notification.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailChange {
    pub counts: Counts,
    pub new_mail: u64,
    pub unread: u64,
}
