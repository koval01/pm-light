//! What the raw headers say about a message: how it was protected and who vouched for the sender.

use crate::pattern::{Pattern, group};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) static AUTH_RESULT: Pattern = Pattern::new(r"(?i)\b(spf|dkim|dmarc)=(\w+)");

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct AuthResult {
    pub method: String,
    pub result: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetails {
    pub encryption: String,
    pub auth: Vec<AuthResult>,
    pub message_id: String,
}

/// Every value of a header. Continuation lines are joined, as RFC 5322 folds long ones.
fn header_values(raw: &str, name: &str) -> Vec<String> {
    let unfolded = raw.replace("\r\n", "\n").replace("\n ", " ").replace("\n\t", " ");
    let prefix = format!("{name}:");
    unfolded
        .lines()
        .filter_map(|line| {
            // Mail decides these bytes, so the cut must be able to fall inside a character.
            let (head, value) = line.split_at_checked(prefix.len())?;
            head.eq_ignore_ascii_case(&prefix).then(|| value.trim().to_string())
        })
        .collect()
}

fn encryption_of(raw: &str) -> &'static str {
    match header_values(raw, "X-Pm-Content-Encryption").first().map(|v| v.to_lowercase()).as_deref() {
        Some("end-to-end") => "endToEnd",
        Some("on-compose") => "onCompose",
        Some("on-delivery") => "onDelivery",
        Some("on-import") => "onImport",
        _ => "stored",
    }
}

fn auth_results(raw: &str) -> Vec<AuthResult> {
    let mut found = BTreeMap::new();
    for value in header_values(raw, "Authentication-Results") {
        for captures in AUTH_RESULT.captures_iter(&value) {
            found.entry(group(&captures, 1).to_lowercase()).or_insert_with(|| group(&captures, 2).to_lowercase());
        }
    }
    found.into_iter().map(|(method, result)| AuthResult { method, result }).collect()
}

#[must_use]
pub fn details_of(raw: &str) -> MessageDetails {
    MessageDetails {
        encryption: encryption_of(raw).to_string(),
        auth: auth_results(raw),
        message_id: header_values(raw, "Message-Id").into_iter().next().unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests;
