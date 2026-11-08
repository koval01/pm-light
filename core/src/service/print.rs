//! Gathers what is to be printed: one message, a thread, or several of either.

use super::{Arc, Deserialize, encoded, Error, Message, now_seconds, on_worker, Result, Serialize, Service, stream, StreamExt, to_text, TryStreamExt, Value};
use crate::{
    print::{self, PrintDocument, PrintItem, PrintLabels, PrintOptions, SealSheet},
    scan,
    seal::{self, Assembly, SealedMail},
};

/// A person printing more than this should export instead, and a mailbox must not be decrypted
/// and laid out by accident.
const MOST_MESSAGES: usize = 100;
const PRINT_CONCURRENCY: usize = 4;
/// `MAILBOX_LABEL_IDS.ALL_MAIL`.
const ALL_MAIL: &str = "5";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PrintScope {
    Message,
    Thread,
    Selected,
    SelectedThreads,
}

/// What a check code says about a sheet of paper.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "result")]
pub enum PrintCheck {
    /// Not a code this app printed.
    Unreadable,
    /// This account holds no such message.
    Unknown,
    /// The code is genuine and the message unchanged. Nothing is known about the paper itself,
    /// so `text` goes with it, for a person to compare with the sheet.
    Matches { sender: String, subject: String, date: u64, text: String },
    /// The message exists, and it no longer says what was printed.
    Differs { sender: String, subject: String, date: u64, text: String },
}

/// What the codes on a sheet add up to. Only a signature that holds says anything about the data,
/// and even then it speaks for the seal, and not for the paper next to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "result")]
pub enum SealCheck {
    Nothing,
    /// Pieces of the text, and no code with a signature to tie them to.
    NoSignature { parts: usize },
    /// The signature holds, and part of the text could not be read.
    Incomplete { signer: String, have: usize, need: usize },
    Damaged,
    /// Proton knows no key for the address named as the signer.
    NoKey { signer: String },
    /// That address did not sign this. Nothing in the code can be believed, so none of it is shown.
    Forged { signer: String },
    Genuine { mail: Box<SealedMail> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetReport {
    pub seal: SealCheck,
    /// The simple code, when a sheet carries one.
    pub print: Option<PrintCheck>,
    /// The sheets themselves, to lay beside the sealed text.
    pub sheets: Vec<String>,
}

impl Service {
    fn seal_of(&self, message: &Message) -> Result<SealSheet> {
        let address_id = message.address_id.clone();
        let signer = self
            .addresses
            .read()
            .iter()
            .find(|address| address.id == address_id)
            .map(|address| address.email.clone())
            .ok_or_else(|| Error::other("This message's address has no key here"))?;
        let fingerprint = self
            .crypto
            .key_fingerprints()
            .into_iter()
            .find(|key| key.primary && key.address_id.as_deref() == Some(address_id.as_str()))
            .map(|key| key.fingerprint)
            .unwrap_or_default();
        let mail = SealedMail {
            signer: signer.clone(),
            sealed_at: now_seconds().saturating_mul(1000),
            sender: message.meta.from.email.clone(),
            recipients: message.meta.to.iter().chain(&message.cc).map(|to| to.email.clone()).collect(),
            date: message.meta.date,
            subject: message.meta.subject.clone(),
            text: to_text(&message.safe_body),
            attachments: message.attachments.iter().map(|file| file.name.clone()).collect(),
        };
        let seal = seal::pack(&mail, |text| self.crypto.clearsign(text, &address_id))?;
        Ok(SealSheet { seal, signer, fingerprint })
    }

    async fn check_seal(&self, codes: &[String]) -> SealCheck {
        let Some(armored) = seal::signed_code(codes) else {
            let parts = codes.iter().filter(|code| seal::is_text_code(code)).count();
            return if parts > 0 { SealCheck::NoSignature { parts } } else { SealCheck::Nothing };
        };
        let Some(signer) = seal::claimed_signer(codes) else { return SealCheck::Damaged };
        let keys = self.sender_keys_of(&signer).await;
        if keys.is_empty() {
            return SealCheck::NoKey { signer };
        }
        let Some(verified) = self.crypto.clearsigned_text(armored, &keys) else {
            log::warn!("a seal's signature did not hold");
            return SealCheck::Forged { signer };
        };
        match seal::assemble(&verified, codes) {
            Assembly::Incomplete { have, need } => SealCheck::Incomplete { signer, have, need },
            Assembly::Damaged => SealCheck::Damaged,
            Assembly::Complete(mail) => SealCheck::Genuine { mail },
        }
    }

    /// Whatever codes were read or pasted, with the pictures they came from.
    pub async fn check_sheets(&self, codes: Vec<String>, sheets: Vec<String>) -> Result<SheetReport> {
        let print = match codes.iter().find(|code| print::parse_check_code(code).is_some()) {
            Some(code) => Some(self.check_print(code).await?),
            None => None,
        };
        Ok(SheetReport { seal: self.check_seal(&codes).await, print, sheets })
    }

    /// Scans are decoded off the async runtime, one file failing does not lose the others.
    pub async fn check_scans(&self, files: Vec<Vec<u8>>) -> Result<SheetReport> {
        let read = on_worker(move || {
            let mut codes = Vec::new();
            let mut sheets = Vec::new();
            let mut unreadable = 0;
            for file in &files {
                match scan::read_sheets(file) {
                    Ok((found, pictures)) => {
                        for code in found {
                            if !codes.contains(&code) {
                                codes.push(code);
                            }
                        }
                        sheets.extend(pictures);
                    }
                    Err(_) => unreadable += 1,
                }
            }
            if sheets.is_empty() && unreadable > 0 {
                return Err(Error::UnreadableScan);
            }
            Ok((codes, sheets))
        });
        let (codes, sheets) = read.await?;
        log::info!("read {} codes from {} scans", codes.len(), sheets.len());
        self.check_sheets(codes, sheets).await
    }

    /// Compares a code from paper with the message in the mailbox. A code cannot see what is written
    /// next to it, so a sheet edited by hand still has a good code: the original goes to the screen.
    pub async fn check_print(&self, code: &str) -> Result<PrintCheck> {
        let Some((id, printed)) = print::parse_check_code(code) else { return Ok(PrintCheck::Unreadable) };
        let message = match self.message(id).await {
            Ok(message) => message,
            Err(Error::Api { status: 404 | 422, .. }) => return Ok(PrintCheck::Unknown),
            Err(error) => return Err(error),
        };
        let (sender, subject) = (message.meta.from.email.clone(), message.meta.subject.clone());
        let (date, text) = (message.meta.date, to_text(&message.safe_body));
        Ok(if print::fingerprint(&message) == printed {
            PrintCheck::Matches { sender, subject, date, text }
        } else {
            PrintCheck::Differs { sender, subject, date, text }
        })
    }

    async fn thread_ids(&self, conversations: Vec<String>) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for conversation in conversations {
            let mut messages = self.thread(&conversation).await?;
            messages.sort_by_key(|message| message.date);
            ids.extend(messages.into_iter().map(|message| message.id));
        }
        Ok(ids)
    }

    async fn print_ids(&self, scope: PrintScope) -> Result<Vec<String>> {
        let (selected, targets) = {
            let view = self.view.lock();
            (view.selected().map(str::to_string), view.targets())
        };
        let conversations = |ids: &[String]| {
            let view = self.view.lock();
            let mut found: Vec<String> =
                ids.iter().filter_map(|id| view.row(id).map(|row| row.conversation_id.clone())).collect();
            found.dedup();
            found
        };
        match scope {
            PrintScope::Message => match selected {
                Some(row) => Ok(vec![self.message_of_row(&row).await?]),
                None => Ok(Vec::new()),
            },
            PrintScope::Selected => Ok(targets),
            PrintScope::Thread => self.thread_ids(conversations(&selected.into_iter().collect::<Vec<_>>())).await,
            PrintScope::SelectedThreads => self.thread_ids(conversations(&targets)).await,
        }
    }

    /// How many messages this sender wrote before a moment. `None` if that could not be learned,
    /// which is not the same as there being none.
    async fn earlier_from(&self, sender: &str, before_seconds: u64) -> Option<u64> {
        let path = format!(
            "/mail/v4/messages?LabelID={ALL_MAIL}&From={}&End={}&Page=0&PageSize=1",
            encoded(sender),
            before_seconds.saturating_sub(1)
        );
        let reply = self.client.get::<Value>(&path).await.ok()?;
        reply.get("Total").and_then(Value::as_u64)
    }

    async fn print_body(&self, message: &Message, images: bool) -> String {
        let urls: Vec<String> = message.protection.images.iter().map(|image| image.url.clone()).collect();
        if images && !urls.is_empty() {
            self.with_remote_images(&message.safe_body, urls).await
        } else {
            message.safe_body.clone()
        }
    }

    pub async fn print_document(
        &self,
        scope: PrintScope,
        options: &PrintOptions,
        labels: &PrintLabels,
    ) -> Result<PrintDocument> {
        let ids = self.print_ids(scope).await?;
        if ids.len() > MOST_MESSAGES {
            return Err(Error::TooManyToPrint { limit: MOST_MESSAGES });
        }
        log::info!("preparing {} messages to print", ids.len());
        let (images, history, sealed) = (options.images, options.history, options.seal);
        let gather = ids.into_iter().map(|id| async move {
            let message = self.message(&id).await?;
            let body = self.print_body(&message, images).await;
            let earlier = if history {
                self.earlier_from(&message.meta.from.email, message.meta.date / 1000).await
            } else {
                None
            };
            let seal = if sealed { Some(self.seal_of(&message)?) } else { None };
            Ok::<_, Error>((message, body, earlier, seal))
        });
        let gathered: Vec<_> = stream::iter(gather).buffered(PRINT_CONCURRENCY).try_collect().await?;
        let items: Vec<PrintItem<'_>> = gathered
            .iter()
            .map(|(message, body, earlier, seal): &(Arc<Message>, String, Option<u64>, Option<SealSheet>)| PrintItem {
                message,
                body: body.clone(),
                earlier: *earlier,
                seal: seal.clone(),
            })
            .collect();
        Ok(print::document(&items, options, labels))
    }
}
