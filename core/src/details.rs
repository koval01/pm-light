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
mod tests {
    use super::*;

    const RAW: &str = "Return-Path: <service@paypal.de>\r\n\
        Authentication-Results: mail.protonmail.ch; dmarc=pass (p=reject dis=none)\r\n header.from=paypal.de\r\n\
        Authentication-Results: mail.protonmail.ch; spf=pass smtp.mailfrom=paypal.de\r\n\
        Authentication-Results: mail.protonmail.ch; dkim=fail (2048-bit key)\r\n\
        Authentication-Results: mail.protonmail.ch; dkim=pass header.d=x\r\n\
        Message-Id: <4F.18@ccg13mail02>\r\n\
        X-Pm-Content-Encryption: on-delivery";

    #[test]
    fn joins_folded_lines() {
        assert!(header_values(RAW, "Authentication-Results")[0].contains("dis=none) header.from"));
    }

    #[test]
    fn does_not_match_a_longer_header_name() {
        assert!(header_values("X-Foo-Bar: 1", "X-Foo").is_empty());
    }

    #[test]
    fn a_header_name_in_tricky_unicode_is_skipped_not_a_crash() {
        // The Kelvin sign lowercases to a plain k, three bytes shorter or longer than it looks.
        assert!(header_values("\u{212a}: 1", "k").is_empty());
        assert_eq!(header_values("\u{212a}ey: 1\nkey: 2", "Key"), ["2"]);
    }

    #[test]
    fn reads_every_mechanism_once_keeping_the_first_verdict() {
        let results = details_of(RAW).auth;
        let pairs: Vec<_> = results.iter().map(|r| (r.method.as_str(), r.result.as_str())).collect();
        assert_eq!(pairs, [("dkim", "fail"), ("dmarc", "pass"), ("spf", "pass")]);
    }

    #[test]
    fn reads_the_encryption_and_message_id() {
        let details = details_of(RAW);
        assert_eq!(details.encryption, "onDelivery");
        assert_eq!(details.message_id, "<4F.18@ccg13mail02>");
    }

    #[test]
    fn only_says_stored_when_unsure() {
        assert_eq!(encryption_of(""), "stored");
        assert_eq!(encryption_of("X-Pm-Content-Encryption: something-new"), "stored");
        assert_eq!(encryption_of("X-Pm-Content-Encryption: end-to-end"), "endToEnd");
    }
}
