//! Mail put into the account as if it had arrived: `/mail/v4/messages/import`, as Proton's Import-Export app and
//! Bridge use it (`go-proton-api`, `ImportMessages`). Each message is encrypted to the first address that sends, as
//! PGP/MIME (`crate::import`), and up to ten go in one multipart request with their metadata. Nothing is sent to
//! anyone: this is how a test account is given mail to check against.

use super::{Error, Form, Part, Result, Service, json, on_worker};
use crate::import::pgp_mime;
use rand::RngCore;
use serde::Deserialize;
use std::fmt::Write as _;

/// As many as Proton takes in one request (`maxImportCount`).
const AT_ONCE: usize = 10;
const INBOX: &str = "0";
/// `MessageFlagReceived`: the mail came in, it was not written here.
const RECEIVED: u32 = 1;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Reply {
    #[serde(default)]
    responses: Vec<Named>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Named {
    name: String,
    response: Outcome,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Outcome {
    code: u32,
    #[serde(rename = "MessageID", default)]
    message_id: String,
}

fn boundary() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

impl Service {
    /// Imports `messages`, each a whole RFC 822 message, into the inbox, `unread` as given for each. Returns the new
    /// messages' ids, in order.
    pub async fn import_mail(&self, messages: &[(String, bool)]) -> Result<Vec<String>> {
        let address = self.addresses().into_iter().next().ok_or_else(|| Error::other("no address to import into"))?;
        let mut ids = Vec::new();
        for batch in messages.chunks(AT_ONCE) {
            let mut wrapped = Vec::new();
            for (literal, _) in batch {
                let (crypto, id, text) = (self.crypto.clone(), address.id.clone(), literal.clone());
                let material = on_worker(move || crypto.encrypt_body(&text, &id, None)).await?;
                wrapped.push(pgp_mime(literal, &material.armored, &boundary()));
            }
            let metadata: serde_json::Map<String, serde_json::Value> = batch
                .iter()
                .enumerate()
                .map(|(index, (_, unread))| {
                    let about = json!({"AddressID": address.id, "LabelIDs": [INBOX], "Unread": u8::from(*unread), "Flags": RECEIVED});
                    (index.to_string(), about)
                })
                .collect();
            let metadata = serde_json::Value::Object(metadata).to_string();
            let form = || {
                let mut form = Form::new();
                for (index, message) in wrapped.iter().enumerate() {
                    let mut bytes = message.clone().into_bytes();
                    bytes.extend_from_slice(b"\r\n");
                    if let Ok(part) = Part::bytes(bytes).file_name(format!("{index}.eml")).mime_str("message/rfc822") {
                        form = form.part(index.to_string(), part);
                    }
                }
                match Part::text(metadata.clone()).mime_str("application/json") {
                    Ok(part) => form.part("Metadata", part),
                    Err(_) => form,
                }
            };
            let reply: Reply = self.client.upload_batch("/mail/v4/messages/import", &form).await?;
            for index in 0..batch.len() {
                let outcome = reply.responses.iter().find(|named| named.name == index.to_string());
                match outcome {
                    Some(named) if named.response.code == 1000 => ids.push(named.response.message_id.clone()),
                    Some(named) => {
                        return Err(Error::other(format!("import refused with code {}", named.response.code)));
                    }
                    None => return Err(Error::other("import gave no answer for a message")),
                }
            }
        }
        log::info!("imported {} messages", ids.len());
        Ok(ids)
    }
}
