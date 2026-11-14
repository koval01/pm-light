//! Sender lists, forwarding, domains and recovery between Proton's shape and ours. The numbers are
//! the constants of Proton's web client.

use super::{LOCATION_BLOCKED, Value, json};
use crate::{
    error::{Error, Result},
    model::{DomainStatus, Forwarding, ForwardingState, RecoveryPhrase, RecoveryStatus, SenderList, SenderRule},
};

const LOCATION_ALLOWED: u8 = 0;
const LOCATION_SPAM: u8 = 4;
const FORWARDING_ENCRYPTED_BIT: u64 = 1;
const VERIFY_GOOD: u64 = 2;
const MX_GOOD: [u64; 2] = [3, 4];
const SPF_GOOD: u64 = 3;
const DKIM_GOOD: u64 = 4;
const DMARC_GOOD: [u64; 2] = [3, 4];

fn field(row: &Value, name: &str) -> String {
    row.get(name).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn count(row: &Value, name: &str) -> u64 {
    row.get(name).and_then(Value::as_u64).unwrap_or(0)
}

#[must_use]
pub fn to_sender_rule(row: &Value) -> Option<SenderRule> {
    let list = match u8::try_from(count(row, "Location")).ok()? {
        LOCATION_ALLOWED => SenderList::Allowed,
        LOCATION_SPAM => SenderList::Spam,
        LOCATION_BLOCKED => SenderList::Blocked,
        _ => return None,
    };
    let email = field(row, "Email");
    let sender = if email.is_empty() { field(row, "Domain") } else { email };
    (!sender.is_empty()).then(|| SenderRule { id: field(row, "ID"), sender, list })
}

/// An address, or a bare domain for everyone at it. Typed by a person, so it is checked here.
pub fn sender_rule_body(sender: &str, list: SenderList) -> Result<Value> {
    let sender = sender.trim();
    let refused = || Error::InvalidAddress(sender.to_string());
    if sender.is_empty() || sender.chars().any(char::is_whitespace) {
        return Err(refused());
    }
    let location = match list {
        SenderList::Allowed => LOCATION_ALLOWED,
        SenderList::Spam => LOCATION_SPAM,
        SenderList::Blocked => LOCATION_BLOCKED,
    };
    match sender.split_once('@') {
        Some((name, domain)) if !name.is_empty() && domain.contains('.') => {
            Ok(json!({"Email": sender, "Location": location}))
        }
        None if sender.contains('.') && !sender.starts_with('.') && !sender.ends_with('.') => {
            Ok(json!({"Domain": sender, "Location": location}))
        }
        _ => Err(refused()),
    }
}

pub fn to_forwarding(row: &Value, outgoing: bool) -> Forwarding {
    Forwarding {
        id: field(row, "ID"),
        address: field(row, if outgoing { "ForwardeeEmail" } else { "ForwarderEmail" }),
        outgoing,
        encrypted: count(row, "Type") & FORWARDING_ENCRYPTED_BIT != 0,
        state: match row.get("State").and_then(Value::as_u64) {
            Some(0) => ForwardingState::Pending,
            Some(1) => ForwardingState::Active,
            Some(2) => ForwardingState::Outdated,
            Some(3) => ForwardingState::Paused,
            Some(4) => ForwardingState::Rejected,
            _ => ForwardingState::Unknown,
        },
    }
}

pub fn to_domain(row: &Value) -> DomainStatus {
    DomainStatus {
        name: field(row, "DomainName"),
        verified: count(row, "VerifyState") == VERIFY_GOOD,
        mx: MX_GOOD.contains(&count(row, "MxState")),
        spf: count(row, "SpfState") == SPF_GOOD,
        dkim: row.pointer("/DKIM/State").and_then(Value::as_u64) == Some(DKIM_GOOD),
        dmarc: DMARC_GOOD.contains(&count(row, "DmarcState")),
    }
}

#[must_use]
pub fn to_recovery(user: &Value, settings: &Value) -> RecoveryStatus {
    let text = |path: &str| settings.pointer(path).and_then(Value::as_str).unwrap_or_default().to_string();
    let on = |path: &str| settings.pointer(path).and_then(Value::as_u64).unwrap_or(0) != 0;
    RecoveryStatus {
        email: text("/Email/Value"),
        email_reset: on("/Email/Reset"),
        phone: text("/Phone/Value"),
        phone_reset: on("/Phone/Reset"),
        phrase: match count(user, "MnemonicStatus") {
            0 => RecoveryPhrase::Off,
            2 => RecoveryPhrase::Outdated,
            3 => RecoveryPhrase::Set,
            _ => RecoveryPhrase::Missing,
        },
        device_backup: on("/DeviceRecovery"),
    }
}
