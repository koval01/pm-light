//! Who may reach the mailbox, where its mail goes, and how the account can be recovered.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SenderList {
    Allowed,
    Spam,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SenderRule {
    pub id: String,
    /// An address or a whole domain.
    pub sender: String,
    pub list: SenderList,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ForwardingState {
    Pending,
    Active,
    Outdated,
    Paused,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Forwarding {
    pub id: String,
    /// The other side: where mail goes, or where it comes from.
    pub address: String,
    pub outgoing: bool,
    pub encrypted: bool,
    pub state: ForwardingState,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[expect(clippy::struct_excessive_bools, reason = "each is one DNS record that either checks out or not")]
pub struct DomainStatus {
    pub name: String,
    pub verified: bool,
    pub mx: bool,
    pub spf: bool,
    pub dkim: bool,
    pub dmarc: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RecoveryPhrase {
    Off,
    Missing,
    Outdated,
    Set,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatus {
    pub email: String,
    pub email_reset: bool,
    pub phone: String,
    pub phone_reset: bool,
    pub phrase: RecoveryPhrase,
    pub device_backup: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    /// Empty for the account's own key.
    pub address: String,
    pub fingerprint: String,
    pub primary: bool,
}
