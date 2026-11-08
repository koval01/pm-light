//! Drafts, attachments and sending.

use super::{Address, api, Attachment, compose, ComposeLabels, ComposeMode, ComposeState, Deserialize, Engine, Error, Form, join_all, json, Message, Method, on_worker, Outgoing, OwnAddress, Part, PasswordProtection, Recipient, RECIPIENT_INTERNAL, Result, RngCore, SavedDraft, send, SendOptions, Service, sieve, SimpleFilter, STANDARD, text, Uploaded, Value};

impl Service {
    /// Holds a file's bytes until the draft is saved, so they cross the IPC boundary only once.
    pub fn stage_file(&self, data: Vec<u8>) -> String {
        let mut id = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut id);
        let id = crate::hex(&id);
        self.staged.lock().insert(id.clone(), data);
        id
    }

    /// A picture as it goes out, when the account wants what the camera wrote left at home.
    pub fn prepared_file(&self, data: Vec<u8>) -> Vec<u8> {
        if self.habits().remove_metadata { crate::image_metadata::without_metadata(data) } else { data }
    }

    pub(super) fn pick_sender(&self, email: Option<&str>) -> Result<Address> {
        // A copy, so that the lock is gone before the keys are asked about under a lock of their own.
        let addresses = self.addresses.read().clone();
        let mut usable = addresses.into_iter().filter(|a| a.can_send && self.crypto.has_keys_for(&a.id));
        let Some(email) = email.filter(|e| !e.is_empty()) else {
            return usable.next().ok_or_else(|| Error::other("No address on this account can send mail"));
        };
        usable
            .find(|a| a.email.eq_ignore_ascii_case(email))
            .ok_or_else(|| Error::other(format!("{email} is not a sending address on this account")))
    }

    pub(super) async fn store_draft(&self, mail: &Outgoing) -> Result<(String, Address, crate::crypto::BodyMaterial)> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(rename = "Message")]
            message: Stored,
        }
        #[derive(Deserialize)]
        struct Stored {
            #[serde(rename = "ID")]
            id: String,
        }
        let sender = self.pick_sender(mail.from.as_deref())?;
        let crypto = self.crypto.clone();
        let (text, address, signed_at) = (mail.body.clone(), sender.id.clone(), mail.deliver_at);
        let material = on_worker(move || crypto.encrypt_body(&text, &address, signed_at)).await?;
        let payload = send::draft_payload(mail, &sender.email, &sender.display_name, &material.armored);
        let reply: Reply = match &mail.draft_id {
            Some(id) => self.client.put(&format!("/mail/v4/messages/{id}"), payload).await?,
            None => self.client.post("/mail/v4/messages", payload).await?,
        };
        Ok((reply.message.id, sender, material))
    }

    pub(super) async fn upload(
        &self,
        mail: &Outgoing,
        draft_id: &str,
        sender: &Address,
    ) -> Result<(Vec<Uploaded>, Vec<Attachment>)> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(rename = "Attachment")]
            attachment: api::ApiAttachment,
        }
        let mut uploaded = Vec::new();
        let mut attachments = Vec::new();
        for file in &mail.files {
            // Kept until the upload succeeds, so that a failed one can be retried.
            let data = self
                .staged
                .lock()
                .get(&file.staged_id)
                .cloned()
                .ok_or_else(|| Error::other(format!("{} is no longer available", file.name)))?;
            let crypto = self.crypto.clone();
            let address = sender.id.clone();
            let material = on_worker(move || crypto.encrypt_attachment(&data, &address)).await?;
            let mime = if file.mime.is_empty() { "application/octet-stream" } else { &file.mime };
            let form = || {
                let blob = |bytes: &[u8]| {
                    Part::bytes(bytes.to_vec()).file_name("blob").mime_str("application/octet-stream")
                };
                let form = Form::new()
                    .text("Filename", file.name.clone())
                    .text("MessageID", draft_id.to_string())
                    .text("MIMEType", mime.to_string())
                    .text("ContentID", "")
                    .text("Disposition", "attachment");
                match (blob(&material.key_packets), blob(&material.data_packet), blob(&material.signature)) {
                    (Ok(keys), Ok(data), Ok(signature)) => {
                        form.part("KeyPackets", keys).part("DataPacket", data).part("Signature", signature)
                    }
                    _ => form,
                }
            };
            let reply: Reply = self.client.upload("/mail/v4/attachments", &form).await?;
            self.staged.lock().remove(&file.staged_id);
            attachments.push(api::to_attachment(&reply.attachment));
            uploaded.push(Uploaded {
                id: reply.attachment.id,
                session_key: material.session_key,
                signed: true,
            });
        }
        Ok((uploaded, attachments))
    }

    pub async fn save_draft(&self, state: &ComposeState) -> Result<SavedDraft> {
        let mail = compose::draft_outgoing(state);
        let (id, sender, _) = self.store_draft(&mail).await?;
        let (_, attachments) = self.upload(&mail, &id, &sender).await?;
        self.forget_message(&id);
        Ok(SavedDraft { id, attachments })
    }

    pub async fn compose(
        &self,
        mode: ComposeMode,
        message_id: Option<&str>,
        labels: &ComposeLabels,
    ) -> Result<ComposeState> {
        let message = match message_id {
            Some(id) => {
                let opened = self.opened.lock().iter().find(|m| m.meta.id == id).cloned();
                Some(match opened {
                    Some(message) => message,
                    None => self.message(id).await?,
                })
            }
            None => None,
        };
        let starts_fresh = !matches!(mode, ComposeMode::Draft);
        let own = self.signing_addresses(&labels.proton_footer);
        let mut state = compose::compose_from(mode, message.as_deref(), &own, labels);
        if self.is_plain_letter(mode, message.as_deref()) {
            return Ok(compose::plain::into_plain(state, mode, message.as_deref(), &own, labels));
        }
        if starts_fresh {
            state.body = compose::styled_first_line(&state.body, &self.habits());
        }
        Ok(state)
    }

    /// A new letter is as the account asks, and any other is of the type of the mail it starts from.
    fn is_plain_letter(&self, mode: ComposeMode, message: Option<&Message>) -> bool {
        let starts_from = message.filter(|_| mode != ComposeMode::New);
        starts_from.map_or_else(|| self.habits().plain_drafts, |m| !m.html)
    }

    /// Whether a letter that starts from nothing is written as text.
    pub fn new_letters_are_plain(&self) -> bool {
        self.is_plain_letter(ComposeMode::New, None)
    }

    pub async fn domain_accepts_mail(&self, domain: &str) -> Result<bool> {
        self.client.dns()?.accepts_mail(domain).await
    }

    /// Each address's signature as a letter carries it, with Proton's line when the account has it.
    fn signing_addresses(&self, footer: &str) -> Vec<OwnAddress> {
        let mut own = self.addresses();
        if self.habits().proton_footer && !footer.is_empty() {
            for address in &mut own {
                address.signature = compose::with_footer(&address.signature, footer);
            }
        }
        own
    }

    /// The body with the signature of the address now sending it.
    pub fn switch_signature(&self, body: &str, email: &str, footer: &str) -> String {
        compose::replace_signature(body, compose::signature_for(&self.signing_addresses(footer), Some(email)))
    }

    /// The same for a letter written as text, which has to name the signature it replaces.
    pub fn switch_plain_signature(&self, body: &str, from: Option<&str>, email: &str, footer: &str) -> String {
        let own = self.signing_addresses(footer);
        compose::plain::replace_signature(body, compose::signature_for(&own, from), compose::signature_for(&own, Some(email)))
    }

    pub fn build_sieve(&self, filter: &SimpleFilter) -> String {
        sieve::build_sieve(filter)
    }

    pub async fn send_composed(&self, state: &ComposeState, options: SendOptions) -> Result<String> {
        self.send(&compose::to_outgoing(state, options)?).await
    }

    pub(super) async fn recipient_key(&self, email: String) -> Result<Recipient> {
        let (kind, keys) = self.public_keys(&email).await?;
        let public_key = (kind == RECIPIENT_INTERNAL)
            .then(|| send::encryption_key_of(&keys).map(str::to_string))
            .flatten();
        Ok(Recipient { email, public_key })
    }

    /// Returns the ID of the sent message, which can still be recalled during the undo window.
    pub async fn send(&self, mail: &Outgoing) -> Result<String> {
        log::info!("sending a message");
        let mut recipients: Vec<String> = Vec::new();
        for email in mail.to.iter().chain(&mail.cc).chain(&mail.bcc) {
            let email = email.trim();
            if !email.is_empty() && !recipients.iter().any(|known| known == email) {
                recipients.push(email.to_string());
            }
        }
        if recipients.is_empty() {
            return Err(Error::other("There is nobody to send this to"));
        }

        let (id, sender, material) = self.store_draft(mail).await?;
        let (mut uploaded, _) = self.upload(mail, &id, &sender).await?;
        for attachment in &mail.existing {
            let packets = STANDARD.decode(&attachment.key_packets).map_err(Error::crypto)?;
            let session_key = self.crypto.session_key(&packets, &sender.id)?;
            uploaded.push(Uploaded { id: attachment.id.clone(), session_key, signed: attachment.signed });
        }
        let keys = join_all(recipients.into_iter().map(|email| self.recipient_key(email))).await;
        let keys = keys.into_iter().collect::<Result<Vec<_>>>()?;

        let protection = match mail.password.as_deref().filter(|p| !p.is_empty()) {
            Some(password) => {
                let modulus: Value = self.client.get("/auth/v4/modulus").await?;
                Some(PasswordProtection {
                    password: password.to_string(),
                    hint: mail.password_hint.clone().unwrap_or_default(),
                    modulus_id: text(&modulus, "ModulusID"),
                    signed_modulus: text(&modulus, "Modulus"),
                })
            }
            None => None,
        };

        let crypto = self.crypto.clone();
        let mime = mail.mime.clone();
        let package = on_worker(move || {
            send::build_package(crypto.as_ref(), &mime, &material, &keys, &uploaded, protection.as_ref())
        })
        .await?;

        let mut body = json!({"Packages": [package]});
        if let Some(at) = mail.deliver_at {
            crate::mapping::put(&mut body, "DeliveryTime", json!(at));
        }
        if let Some(save) = mail.auto_save_contacts {
            crate::mapping::put(&mut body, "AutoSaveContacts", json!(u8::from(save)));
        }
        if let Some(delay) = mail.delay_seconds.filter(|d| *d > 0) {
            crate::mapping::put(&mut body, "DelaySeconds", json!(delay));
        }
        if let Some(expires) = mail.expires_in.filter(|e| *e > 0) {
            crate::mapping::put(&mut body, "ExpiresIn", json!(expires));
        }
        self.client.act_with(Method::POST, &format!("/mail/v4/messages/{id}"), body).await?;
        Ok(id)
    }

    pub async fn cancel_send(&self, id: &str) -> Result<()> {
        self.client.act(Method::POST, &format!("/mail/v4/messages/{id}/cancel_send")).await
    }
}
