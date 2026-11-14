//! Reading the mailbox: folders, lists, messages and what they carry.

use super::{
    Arc, Attachment, Counts, Deserialize, Engine, Error, Folder, KEPT_THREADS, ListOptions, MIME_CACHE_MESSAGES,
    MailChange, MailProtection, MailStore, Message, MessageMeta, MimeAttachment, OPENED_AHEAD_TOGETHER, OPENED_BYTES,
    OPENED_MESSAGES, Ordering, PathBuf, PublicKeyRow, Result, STANDARD, Service, StreamExt, Value, api, body_text,
    details, encoded, html, is_inline_image, mail_store, on_worker, parse_mime, safety, send, stream, text,
    to_data_url,
};
use crate::view::StoredList;
use std::path::Path;

/// Never grouped, as `ALWAYS_MESSAGE_LABELS` in Proton's client.
const ALWAYS_MESSAGES: [&str; 2] = ["drafts", "sent"];
const MAIL: &str = "mail";
const LIST: &str = "list";

const MIME_FILES: &str = "mime-files";

/// A page of a folder, and how many rows the folder holds as Proton counts them, if it said.
pub struct Listed {
    pub rows: Vec<MessageMeta>,
    pub total: Option<u64>,
}

impl Service {
    pub async fn folders(&self) -> Result<Vec<Folder>> {
        let fetch = |kind: &'static str, kind_id: u8| async move {
            let reply: Value = self.client.get(&format!("/core/v4/labels?Type={kind_id}")).await?;
            Ok::<_, Error>(reply.get("Labels").and_then(Value::as_array).map_or_default(|labels| {
                labels
                    .iter()
                    .map(|l| Folder { id: text(l, "ID"), name: text(l, "Name"), kind, color: Some(text(l, "Color")) })
                    .collect::<Vec<_>>()
            }))
        };
        let (folders, labels) =
            futures::try_join!(fetch("folder", api::LABEL_TYPE_FOLDER), fetch("label", api::LABEL_TYPE_LABEL))?;
        Ok(api::system_folders().into_iter().chain(folders).chain(labels).collect())
    }

    pub async fn counts(&self) -> Result<Counts> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Reply {
            counts: Vec<api::ApiCount>,
        }
        let path = if self.groups_mail() { "/mail/v4/conversations/count" } else { "/mail/v4/messages/count" };
        let reply: Reply = self.client.get(path).await?;
        let counts = api::to_counts(&reply.counts);
        *self.last_counts.lock() = Some(counts.clone());
        Ok(counts)
    }

    /// What arrived since the last look; nothing when nothing changed.
    pub async fn check_mail(&self) -> Result<Option<MailChange>> {
        let before = self.last_counts.lock().clone();
        let after = self.counts().await?;
        if before.as_ref() == Some(&after) {
            return Ok(None);
        }
        self.threads.lock().clear();
        Ok(Some(MailChange {
            new_mail: before.as_ref().map_or(0, |b| api::new_mail_count(b, &after)),
            unread: after.get("inbox").map_or(0, |c| c.unread),
            counts: after,
        }))
    }

    fn groups_mail(&self) -> bool {
        self.habits().conversations
    }

    /// As `isConversationMode` in Proton's client: a search finds messages, and so do these folders.
    fn lists_conversations(&self, folder: &str, options: &ListOptions) -> bool {
        let searching =
            [&options.keyword, &options.from, &options.to, &options.subject].iter().any(|field| field.is_some());
        self.groups_mail() && !searching && !ALWAYS_MESSAGES.contains(&folder)
    }

    async fn list_conversations(&self, folder: &str, options: &ListOptions) -> Result<Listed> {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "PascalCase", default)]
        struct Reply {
            conversations: Vec<api::ApiConversation>,
            total: Option<u64>,
        }
        let label = api::listed_label(folder, &self.habits());
        let reply: Reply =
            self.client.get(&format!("/mail/v4/conversations?{}", api::list_query(label, options))).await?;
        let rows = reply.conversations.iter().map(|row| api::conversation_row(row, folder)).collect();
        Ok(Listed { rows, total: reply.total })
    }

    pub async fn list(&self, folder: &str, options: &ListOptions) -> Result<Vec<MessageMeta>> {
        Ok(self.list_counted(folder, options).await?.rows)
    }

    /// A page of a folder, and how many rows the whole folder holds with the same filters, as Proton counts them.
    pub async fn list_counted(&self, folder: &str, options: &ListOptions) -> Result<Listed> {
        #[derive(Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct Reply {
            messages: Vec<api::ApiMessageMeta>,
            #[serde(default)]
            total: Option<u64>,
        }
        if self.lists_conversations(folder, options) {
            return self.list_conversations(folder, options).await;
        }
        let label = api::listed_label(folder, &self.habits());
        let path = format!("/mail/v4/messages?{}", api::list_query(label, options));
        let reply: Reply = self.client.get(&path).await?;
        let rows = reply.messages.iter().map(|m| api::to_meta(m, Some(folder))).collect();
        Ok(Listed { rows, total: reply.total })
    }

    pub async fn thread(&self, conversation_id: &str) -> Result<Vec<MessageMeta>> {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "PascalCase", default)]
        struct Reply {
            messages: Vec<api::ApiMessageMeta>,
        }
        if let Some((_, kept)) = self.threads.lock().iter().find(|(id, _)| id == conversation_id) {
            return Ok(kept.clone());
        }
        let reply: Reply = self.client.get(&format!("/mail/v4/conversations/{conversation_id}")).await?;
        let rows: Vec<MessageMeta> = reply.messages.iter().map(|m| api::to_meta(m, None)).collect();
        let mut threads = self.threads.lock();
        threads.push_back((conversation_id.to_string(), rows.clone()));
        while threads.len() > KEPT_THREADS {
            threads.pop_front();
        }
        drop(threads);
        Ok(rows)
    }

    pub(super) async fn public_keys(&self, email: &str) -> Result<(u8, Vec<PublicKeyRow>)> {
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "PascalCase", default)]
        struct Reply {
            recipient_type: u8,
            keys: Vec<PublicKeyRow>,
        }
        let reply: Reply = self.client.get(&format!("/core/v4/keys?Email={}", encoded(email))).await?;
        Ok((reply.recipient_type, reply.keys))
    }

    /// Who may vouch for this sender. A failed lookup means "unknown", not "forged".
    pub(super) async fn sender_keys_of(&self, email: &str) -> Vec<String> {
        let key = email.to_lowercase();
        if let Some(known) = self.sender_keys.lock().get(&key) {
            return known.clone();
        }
        let keys = self.public_keys(email).await.map(|(_, keys)| send::trusted_keys_of(&keys));
        let keys = keys.unwrap_or_default();
        self.sender_keys.lock().insert(key, keys.clone());
        keys
    }

    /// A message read before. Its body cannot change, only the flags on it, and those come from
    /// the list. PGP/MIME files live elsewhere: without them the message has to be read again.
    pub(super) fn kept_message(&self, id: &str) -> Option<Arc<Message>> {
        let row = self.view.lock().row(id).cloned();
        let files_gone = || !self.mime_files.lock().iter().any(|(message, _)| message == id);
        let mut opened = self.opened.lock();
        let at = opened.iter().position(|known| known.meta.id == id)?;
        let mut kept = opened.remove(at)?;
        if kept.attachments.iter().any(|file| file.id.starts_with("mime-")) && files_gone() {
            return None;
        }
        if let Some(row) = row.filter(|row| row.unread != kept.meta.unread || row.labels != kept.meta.labels) {
            let meta =
                MessageMeta { unread: row.unread, starred: row.starred, labels: row.labels, ..kept.meta.clone() };
            kept = Arc::new(Message { meta, ..(*kept).clone() });
        }
        opened.push_back(kept.clone());
        drop(opened);
        Some(kept)
    }

    pub(super) fn keep_message(&self, message: Arc<Message>) {
        // Locked or signed out meanwhile: nothing of the account may come back into memory.
        if self.client.session().is_none() {
            return;
        }
        let weight = |m: &Message| m.body.len() + m.safe_body.len() + m.raw_headers.len();
        let mut opened = self.opened.lock();
        opened.retain(|known| known.meta.id != message.meta.id);
        opened.push_back(message);
        let mut total: usize = opened.iter().map(|m| weight(m)).sum();
        while opened.len() > 1 && (opened.len() > OPENED_MESSAGES || total > OPENED_BYTES) {
            total -= opened.pop_front().map_or(0, |old| weight(&old));
        }
        drop(opened);
    }

    pub(super) fn forget_message(&self, id: &str) {
        self.opened.lock().retain(|known| known.meta.id != id);
        if let Some(store) = self.mail_store.read().as_ref() {
            store.forget(MAIL, id);
        }
    }

    /// `None` turns the disk off, and takes what is on it along.
    pub fn use_mail_dir(&self, dir: Option<PathBuf>, was: Option<&Path>) {
        if dir.is_none()
            && let Some(old) = was
        {
            mail_store::erase(old);
        }
        *self.mail_dir.lock() = dir;
        *self.mail_store.write() = None;
        if self.key_secret.lock().is_some() {
            self.open_mail_store();
        }
    }

    pub(super) fn open_mail_store(&self) {
        let Some(dir) = self.mail_dir.lock().clone() else { return };
        let key = self.store_key.lock().get_or_insert_with(mail_store::new_key).clone();
        let Some(store) = MailStore::open(&dir, key).map(Arc::new) else { return };
        *self.mail_store.write() = Some(store.clone());
        drop(tokio::task::spawn_blocking(move || store.prune()));
    }

    pub(super) fn store(&self) -> Option<Arc<MailStore>> {
        self.mail_store.read().clone()
    }

    /// Written behind the reader's back: nobody waits for the disk to see a message.
    fn store_message(&self, message: &Arc<Message>) {
        let Some(store) = self.store() else { return };
        let message = message.clone();
        let files =
            self.mime_files.lock().iter().find(|(id, _)| *id == message.meta.id).map(|(_, files)| files.clone());
        drop(tokio::task::spawn_blocking(move || {
            if let Some(files) = files {
                store.put(MIME_FILES, &message.meta.id, &files);
            }
            store.put(MAIL, &message.meta.id, &*message);
        }));
    }

    pub(super) async fn stored_message(&self, id: &str) -> Option<Arc<Message>> {
        let store = self.store()?;
        let wanted = id.to_string();
        let read = on_worker(move || {
            let message: Option<Message> = store.get(MAIL, &wanted);
            let packed = message
                .as_ref()
                .is_some_and(|message| message.attachments.iter().any(|file| file.id.starts_with("mime-")));
            let files: Option<Vec<MimeAttachment>> = if packed { store.get(MIME_FILES, &wanted) } else { None };
            Ok((message, packed, files))
        });
        let (message, packed, files) = read.await.ok()?;
        if packed {
            // Without its files a message is asked for again, or they could not be saved later.
            self.remember_mime_files(id, files?);
        }
        let message = message.filter(|message| !self.must_ask_again(message))?;
        self.keep_message(Arc::new(message));
        self.kept_message(id)
    }

    /// What is shown of this mail depends on who sent it, and only Proton can say. Mail stored before
    /// that was kept is read again once, and only by an account for which it makes a difference.
    pub(super) fn must_ask_again(&self, message: &Message) -> bool {
        message.from_proton.is_none() && self.habits().hide_embedded
    }

    pub(super) fn store_list(&self, folder: &str, list: &StoredList) {
        if let Some(store) = self.store() {
            let (folder, list) = (folder.to_string(), list.clone());
            drop(tokio::task::spawn_blocking(move || store.put(LIST, &folder, &list)));
        }
    }

    pub(super) async fn stored_list(&self, folder: &str) -> Option<StoredList> {
        let store = self.store()?;
        let wanted = folder.to_string();
        on_worker(move || Ok(store.get(LIST, &wanted))).await.ok()?
    }

    /// Reads mail before the click, so that the click costs nothing. Fetching a message marks
    /// nothing as read and loads nothing remote: only the person's own actions do that.
    /// The message a row opens. Only a conversation has to ask, and it asks once.
    pub async fn message_of_row(&self, id: &str) -> Result<String> {
        if !self.is_conversation_row(id) {
            return Ok(id.to_string());
        }
        let thread = self.thread(id).await?;
        let folder = self.view.lock().folder.clone();
        api::message_to_open(&thread, &folder).ok_or_else(|| Error::other("An empty conversation"))
    }

    pub fn is_conversation_row(&self, id: &str) -> bool {
        self.view.lock().row(id).is_some_and(|row| row.messages > 0)
    }

    pub fn has_row(&self, id: &str) -> bool {
        self.view.lock().row(id).is_some()
    }

    pub(super) fn open_ahead(&self, ids: Vec<String>) {
        let Some(me) = self.me.upgrade() else { return };
        let wanted: Vec<String> = {
            let opened = self.opened.lock();
            ids.into_iter().filter(|id| !opened.iter().any(|known| known.meta.id == *id)).collect()
        };
        if wanted.is_empty() || self.opening_ahead.swap(true, Ordering::Relaxed) {
            return;
        }
        tokio::spawn(async move {
            log::debug!("reading {} messages ahead", wanted.len());
            let reads: Vec<_> = wanted
                .into_iter()
                .map(|id| {
                    let me = me.clone();
                    async move {
                        let Ok(id) = me.message_of_row(&id).await else { return false };
                        me.message(&id).await.is_ok()
                    }
                })
                .collect();
            let done: Vec<bool> = stream::iter(reads).buffer_unordered(OPENED_AHEAD_TOGETHER).collect().await;
            log::debug!("read {} of them ahead", done.iter().filter(|read| **read).count());
            me.opening_ahead.store(false, Ordering::Relaxed);
        });
    }

    pub async fn message(&self, id: &str) -> Result<Arc<Message>> {
        #[derive(Deserialize)]
        struct Reply {
            #[serde(rename = "Message")]
            message: api::ApiMessage,
        }
        if let Some(kept) = self.kept_message(id) {
            return Ok(kept);
        }
        if let Some(stored) = self.stored_message(id).await {
            return Ok(stored);
        }
        let m = self.client.get::<Reply>(&format!("/mail/v4/messages/{id}")).await?.message;
        let senders = self.sender_keys_of(&m.meta.sender.address).await;

        let crypto = self.crypto.clone();
        let (armored, address_id) = (m.body.clone(), m.meta.address_id.clone());
        let opened = on_worker(move || crypto.decrypt_message(&armored, &address_id, &senders)).await?;

        let mut attachments: Vec<_> = m.attachments.iter().map(api::to_attachment).collect();
        let (body, is_html) = if m.mime_type.starts_with("multipart/") {
            let data = opened.data;
            let parsed = on_worker(move || Ok(parse_mime(&data))).await?;
            attachments.extend(parsed.attachments.iter().enumerate().map(|(i, file)| Attachment {
                id: format!("mime-{i}"),
                name: file.name.clone(),
                size: file.data.len() as u64,
                mime: file.mime.clone(),
                key_packets: String::new(),
                signed: false,
                inline: file.inline,
                content_id: file.content_id.clone(),
                previewable: safety::can_preview(&file.mime),
            }));
            self.remember_mime_files(id, parsed.attachments);
            match parsed.html {
                Some(html) => (html, true),
                None => (parsed.text.unwrap_or_default(), false),
            }
        } else {
            (body_text(&opened.data), m.mime_type == "text/html")
        };

        let (safe_body, protection) = if is_html {
            let inlined = self.inline_images(&body, &attachments, id, &m.meta.address_id).await;
            // On a worker: a panic on hostile input then costs this one message, not the app.
            let protected = on_worker(move || Ok(html::protect(&inlined))).await?;
            (protected.html, protected.protection)
        } else {
            (String::new(), MailProtection::default())
        };

        let message = Arc::new(Message {
            meta: api::to_meta(&m.meta, None),
            address_id: m.meta.address_id.clone(),
            size: m.size,
            signature: opened.signature,
            details: details::details_of(m.header.as_deref().unwrap_or_default()),
            raw_headers: m.header.clone().unwrap_or_default(),
            unsubscribe: api::to_unsubscribe(m.unsubscribe_methods.as_ref()),
            receipt_requested: api::needs_read_receipt(m.meta.flags, &m.parsed_headers),
            cc: api::cc_of(&m.meta),
            bcc: api::bcc_of(&m.meta),
            reply_to: api::reply_to_of(&m),
            body,
            safe_body,
            protection,
            html: is_html,
            attachments,
            from_proton: Some(m.meta.sender.is_proton == 1),
        });
        self.keep_message(message.clone());
        self.store_message(&message);
        Ok(message)
    }

    pub(super) fn remember_mime_files(&self, id: &str, files: Vec<MimeAttachment>) {
        let mut cache = self.mime_files.lock();
        cache.retain(|(message, _)| message != id);
        if !files.is_empty() {
            cache.push_back((id.to_string(), files));
        }
        while cache.len() > MIME_CACHE_MESSAGES {
            cache.pop_front();
        }
        drop(cache);
    }

    pub(super) async fn inline_images(
        &self,
        html: &str,
        attachments: &[Attachment],
        message_id: &str,
        address_id: &str,
    ) -> String {
        let mut out = html.to_string();
        for attachment in attachments {
            let Some(content_id) = &attachment.content_id else { continue };
            let reference = format!("cid:{content_id}");
            if !is_inline_image(&attachment.mime) || !out.contains(&reference) {
                continue;
            }
            if !usize::try_from(attachment.size).is_ok_and(|size| size <= safety::MAX_IMAGE_BYTES) {
                continue;
            }
            if let Ok(data) = self.open_attachment(message_id, attachment, address_id).await
                && safety::is_displayable_image(&attachment.mime, &data)
            {
                out = out.replace(&reference, &to_data_url(&attachment.mime, &data));
            }
        }
        out
    }

    pub(super) async fn open_attachment(&self, message_id: &str, a: &Attachment, address_id: &str) -> Result<Vec<u8>> {
        if let Some(index) = a.id.strip_prefix("mime-").and_then(|i| i.parse::<usize>().ok()) {
            let data = self
                .mime_files
                .lock()
                .iter()
                .find(|(id, _)| id == message_id)
                .and_then(|(_, files)| files.get(index))
                .map(|file| file.data.clone());
            return data.ok_or_else(|| Error::other("Open the message again to get this file"));
        }
        let (data, _) = self.client.fetch(&format!("/mail/v4/attachments/{}", a.id)).await?;
        let packets = STANDARD.decode(&a.key_packets).map_err(Error::crypto)?;
        let crypto = self.crypto.clone();
        let address = address_id.to_string();
        on_worker(move || crypto.decrypt_attachment(&packets, &data, &address)).await
    }

    pub async fn attachment(&self, message_id: &str, a: &Attachment, address_id: &str) -> Result<Vec<u8>> {
        self.open_attachment(message_id, a, address_id).await
    }

    /// The whole message as one file. Every attachment is fetched and decrypted for it.
    pub async fn export_eml(&self, message: &Message) -> Result<Vec<u8>> {
        let mut files = Vec::with_capacity(message.attachments.len());
        for file in &message.attachments {
            let data = self.open_attachment(&message.meta.id, file, &message.address_id).await?;
            files.push((file.clone(), data));
        }
        Ok(crate::eml::build(message, &files))
    }
}
