//! The body is encrypted once with a random session key; each recipient then gets that key wrapped
//! to their own public key. Follows go-proton-api and Proton's web client.

use crate::{
    crypto::{BodyMaterial, Crypto, RawSessionKey},
    error::{Error, Result},
    model::Outgoing,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{Map, Value, json};

/// `PACKAGE_TYPE` in Proton's web client; a package's Type is the OR of its recipients' schemes.
pub const SCHEME_INTERNAL: u8 = 1;
pub const SCHEME_ENCRYPTED_OUTSIDE: u8 = 2;
pub const SCHEME_CLEAR: u8 = 4;
/// `PACKAGE_SIGNATURES_MODE`: 1 means the body and every attachment are signed.
const SIGNATURES_NONE: u8 = 0;
const SIGNATURES_ATTACHMENTS: u8 = 1;
const FLAG_RECEIPT_REQUEST: u64 = 1 << 16;
const KEY_NOT_COMPROMISED: u32 = 1;
const KEY_NOT_OBSOLETE: u32 = 2;
const KEY_EMAIL_NO_ENCRYPT: u32 = 4;
const AUTH_VERSION: u8 = 4;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PublicKeyRow {
    pub flags: u32,
    pub public_key: String,
}

/// First key that can encrypt mail: not obsolete, and not barred from email encryption.
#[must_use]
pub fn encryption_key_of(keys: &[PublicKeyRow]) -> Option<&str> {
    keys.iter()
        .find(|k| k.flags & KEY_NOT_OBSOLETE != 0 && k.flags & KEY_EMAIL_NO_ENCRYPT == 0)
        .map(|k| k.public_key.as_str())
}

/// Keys that may vouch for a signature: an obsolete key still can, a compromised one cannot.
#[must_use]
pub fn trusted_keys_of(keys: &[PublicKeyRow]) -> Vec<String> {
    keys.iter().filter(|k| k.flags & KEY_NOT_COMPROMISED != 0).map(|k| k.public_key.clone()).collect()
}

pub struct Recipient {
    pub email: String,
    /// Absent for recipients who get ordinary, unencrypted mail.
    pub public_key: Option<String>,
}

pub struct Uploaded {
    pub id: String,
    pub session_key: RawSessionKey,
    pub signed: bool,
}

/// For someone outside Proton, with a password they got by other means.
pub struct PasswordProtection {
    pub password: String,
    pub hint: String,
    pub modulus_id: String,
    pub signed_modulus: String,
}

fn list(emails: &[String]) -> Value {
    Value::Array(emails.iter().map(|email| json!({"Address": email, "Name": ""})).collect())
}

#[must_use]
pub fn draft_payload(mail: &Outgoing, email: &str, display_name: &str, armored_body: &str) -> Value {
    let mut message = json!({
        "Subject": mail.subject,
        "Sender": {"Address": email, "Name": display_name},
        "ToList": list(&mail.to),
        "CCList": list(&mail.cc),
        "BCCList": list(&mail.bcc),
        "Body": armored_body,
        "MIMEType": mail.mime,
        "Unread": 0,
    });
    if mail.read_receipt == Some(true) {
        crate::mapping::put(&mut message, "Flags", json!(FLAG_RECEIPT_REQUEST));
    }
    let mut payload = json!({"Message": message, "AttachmentKeyPackets": []});
    if let Some(parent) = &mail.parent_id {
        crate::mapping::put(&mut payload, "ParentID", json!(parent));
        crate::mapping::put(&mut payload, "Action", json!(mail.action.unwrap_or(0)));
    }
    payload
}

fn signature_mode(attachments: &[Uploaded]) -> u8 {
    if attachments.iter().all(|a| a.signed) { SIGNATURES_ATTACHMENTS } else { SIGNATURES_NONE }
}

fn attachment_packets(
    attachments: &[Uploaded],
    mut wrap: impl FnMut(&RawSessionKey) -> Result<Vec<u8>>,
) -> Result<Map<String, Value>> {
    attachments.iter().map(|a| Ok((a.id.clone(), json!(STANDARD.encode(wrap(&a.session_key)?))))).collect()
}

fn internal_entry(
    crypto: &dyn Crypto,
    public_key: &str,
    body: &BodyMaterial,
    attachments: &[Uploaded],
) -> Result<Value> {
    let packets = attachment_packets(attachments, |key| crypto.wrap_for_key(key, public_key))?;
    let mut entry = json!({
        "Type": SCHEME_INTERNAL,
        "Signature": signature_mode(attachments),
        "BodyKeyPacket": STANDARD.encode(crypto.wrap_for_key(&body.session_key, public_key)?),
    });
    if !packets.is_empty() {
        crate::mapping::put(&mut entry, "AttachmentKeyPackets", Value::Object(packets));
    }
    Ok(entry)
}

fn protected_entry(
    crypto: &dyn Crypto,
    protection: &PasswordProtection,
    body: &BodyMaterial,
    attachments: &[Uploaded],
) -> Result<Value> {
    let password = &protection.password;
    let mut token = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut token);
    let token = STANDARD.encode(token);
    let (salt, verifier) = crypto.password_verifier(password, &protection.signed_modulus)?;
    let packets = attachment_packets(attachments, |key| crypto.wrap_for_password(key, password))?;
    let mut entry = json!({
        "Auth": {
            "Version": AUTH_VERSION,
            "ModulusID": protection.modulus_id,
            "Salt": salt,
            "Verifier": verifier,
        },
        "Type": SCHEME_ENCRYPTED_OUTSIDE,
        "PasswordHint": protection.hint,
        "EncToken": crypto.encrypt_for_password(&token, password)?,
        "Token": token,
        "Signature": signature_mode(attachments),
        "BodyKeyPacket": STANDARD.encode(crypto.wrap_for_password(&body.session_key, password)?),
    });
    if !packets.is_empty() {
        crate::mapping::put(&mut entry, "AttachmentKeyPackets", Value::Object(packets));
    }
    Ok(entry)
}

fn session_key_field(key: &RawSessionKey) -> Value {
    json!({"Key": STANDARD.encode(&key.key), "Algorithm": key.algorithm})
}

/// One package per MIME type, shared by everyone.
///
/// Inside Proton the session key is wrapped to each recipient; for the others the package carries it in the clear,
/// so that Proton's servers can deliver ordinary mail.
pub fn build_package(
    crypto: &dyn Crypto,
    mime: &str,
    body: &BodyMaterial,
    recipients: &[Recipient],
    attachments: &[Uploaded],
    protection: Option<&PasswordProtection>,
) -> Result<Value> {
    if recipients.is_empty() {
        return Err(Error::other("There is nobody to send this to"));
    }
    let mut addresses = Map::new();
    let mut scheme = 0;
    for recipient in recipients {
        let (entry, kind) = match (&recipient.public_key, protection) {
            (Some(key), _) => (internal_entry(crypto, key, body, attachments)?, SCHEME_INTERNAL),
            (None, Some(protection)) => {
                (protected_entry(crypto, protection, body, attachments)?, SCHEME_ENCRYPTED_OUTSIDE)
            }
            (None, None) => (json!({"Type": SCHEME_CLEAR, "Signature": SIGNATURES_NONE}), SCHEME_CLEAR),
        };
        addresses.insert(recipient.email.clone(), entry);
        scheme |= kind;
    }
    let mut package = json!({
        "Addresses": addresses,
        "Type": scheme,
        "MIMEType": mime,
        "Body": STANDARD.encode(&body.data_packet),
    });
    if scheme & SCHEME_CLEAR != 0 {
        crate::mapping::put(&mut package, "BodyKey", session_key_field(&body.session_key));
        let keys: Map<String, Value> =
            attachments.iter().map(|a| (a.id.clone(), session_key_field(&a.session_key))).collect();
        crate::mapping::put(&mut package, "AttachmentKeys", Value::Object(keys));
    }
    Ok(package)
}

#[cfg(test)]
mod tests;
