//! What the interface can ask for. Owns the session and the keys: neither ever reaches the page.

mod account;
mod changes;
mod contacts;
mod images;
mod import;
mod lock;
mod mailbox;
mod print;
mod programs;
mod protection;
mod sending;
mod sensitive;
mod settings;
mod sign_in;
mod view;

use crate::{
    client::{Client, Ending, Session, SessionChange},
    compose::{self, ComposeLabels, ComposeMode, ComposeState, SendOptions},
    crypto::{AddressInput, AuthInfo, Crypto, KeySaltRow, engine},
    details,
    error::{Error, HumanVerification, Link, Result},
    html, mapping as api,
    mime::{MimeAttachment, parse_mime},
    model::{Storage, TimeSettings, TimeSetting, Account, Billing, AccountSession, AuthLogEntry, MessageChanges, ContactEmail, Attachment, Message, Latency, Folder, Counts, MailChange, ListOptions, MessageMeta, MailProtection, Referrals, SmtpToken, Organization, Outgoing, SavedDraft, OwnAddress, MailSettings, Habits, UserChoices, AutoReply, MailFilter, FilterDraft},
    safety,
    send::{self, PasswordProtection, PublicKeyRow, Recipient, Uploaded},
    sieve::{self, SimpleFilter},
    text::{escape, to_text},
    vault, vcard,
};
use crate::mail_store::{self, MailStore, StoreKey};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::{StreamExt, TryStreamExt, future::join_all, stream};
use parking_lot::{Mutex, RwLock};
use proton_crypto_account::keys::LockedKey;
use rand::RngCore;
use reqwest::{
    Method,
    multipart::{Form, Part},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::form_urlencoded;
use zeroize::Zeroizing;

const RECIPIENT_INTERNAL: u8 = 1;
const AUTH_LOG_ENTRIES: u32 = 20;
/// `CONTACT_CARD_TYPE`: signed but readable by the server, which is all a name and address need.
const CARD_SIGNED: u8 = 2;
const FILTER_ENABLED: u64 = 1;
const TWO_FACTOR_TOTP: u64 = 1;
const CONTACTS_PAGE: usize = 1000;
const IMAGE_CONCURRENCY: usize = 6;
/// Files inside recently opened PGP/MIME messages. Proton does not store these separately.
const MIME_CACHE_MESSAGES: usize = 8;
/// The disk holds the rest: memory only has to make going back and forth instant.
const OPENED_MESSAGES: usize = 16;
/// Inline images make a single message large, so the count alone does not bound the memory.
const OPENED_BYTES: usize = 16 * 1024 * 1024;
const OPENED_AHEAD_TOGETHER: usize = 2;
const KEPT_THREADS: usize = 64;

fn encoded(value: &str) -> String {
    form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn is_inline_image(mime: &str) -> bool {
    // Bytes, because a hostile type may put a multi-byte character across this cut.
    let Some((kind, subtype)) = mime.as_bytes().split_at_checked(6) else { return false };
    kind.eq_ignore_ascii_case(b"image/")
        && !subtype.is_empty()
        && subtype.iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-'))
}

/// Message text as Proton's web client normalises it.
fn body_text(data: &[u8]) -> String {
    String::from_utf8_lossy(data).replace("\r\n", "\n").replace('\u{a0}', " ")
}

fn to_data_url(mime: &str, data: &[u8]) -> String {
    format!("data:{mime};base64,{}", STANDARD.encode(data))
}

/// Keeps the session in the system's secret store.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> Option<String>;
    fn save(&self, value: &str);
    fn clear(&self);
    /// The count of wrong app passwords, which must outlive a restart.
    fn load_attempts(&self) -> Option<String> {
        None
    }
    fn save_attempts(&self, _: &str) {}
    fn clear_attempts(&self) {}
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedSession {
    session: Session,
    /// Enough to read mail, not to log in.
    key_secret: Zeroizing<String>,
    /// Opens the mail kept on disk. Here it is sealed with the session, and goes with it.
    #[serde(default)]
    store_key: Option<Zeroizing<String>>,
}

/// What the start of the app is busy with, so that a wait has a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// The system may first ask the person for access to its keychain.
    Session,
    Connecting,
    Keys,
}

pub type StageListener = Box<dyn Fn(Stage) + Send + Sync>;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "step", rename_all = "lowercase")]
pub enum AuthStep {
    Credentials,
    Verify { challenge: HumanVerification },
    Totp,
    Mailbox,
    Ready,
    /// A saved session waits behind the app password.
    Locked,
    /// A saved session waits for Proton. Nothing has been lost, and Rust keeps trying.
    Waiting { link: Link },
}

#[derive(Clone)]
struct Address {
    id: String,
    email: String,
    display_name: String,
    signature: String,
    can_send: bool,
}

#[derive(Default)]
struct Login {
    username: String,
    password: Zeroizing<String>,
    challenge: Option<HumanVerification>,
}

pub struct Service {
    me: Weak<Self>,
    client: Arc<Client>,
    crypto: Arc<dyn Crypto>,
    store: Arc<dyn SessionStore>,
    key_secret: Arc<Mutex<Option<Zeroizing<Vec<u8>>>>>,
    addresses: RwLock<Vec<Address>>,
    login: Mutex<Login>,
    sender_keys: Mutex<HashMap<String, Vec<String>>>,
    mime_files: Mutex<VecDeque<(String, Vec<MimeAttachment>)>>,
    /// What the reader shows: a reply starts from it without decrypting it again.
    opened: Mutex<VecDeque<Arc<Message>>>,
    /// Threads read lately, until a change in the mailbox could have touched them.
    threads: Mutex<VecDeque<(String, Vec<MessageMeta>)>>,
    logos: Mutex<HashMap<String, Option<String>>>,
    staged: Mutex<HashMap<String, Vec<u8>>>,
    last_counts: Mutex<Option<Counts>>,
    lock: Mutex<LockState>,
    attempts: Mutex<Option<vault::Attempts>>,
    /// One password at a time: guesses sent side by side would all get past the freeze.
    password_gate: tokio::sync::Mutex<()>,
    view: Mutex<crate::view::View>,
    view_listener: Mutex<Option<view::ViewListener>>,
    stage_listener: Mutex<Option<StageListener>>,
    mail_dir: Mutex<Option<PathBuf>>,
    mail_store: RwLock<Option<Arc<MailStore>>>,
    store_key: Mutex<Option<StoreKey>>,
    /// One run at a time: mail read ahead must never compete with the mail a person clicked.
    opening_ahead: AtomicBool,
    /// The account's own setting, kept from when it was last read.
    next_on_move: AtomicBool,
    habits: RwLock<Habits>,
}

/// While set, whatever is saved is sealed under this key. Gone from memory whenever the app locks.
#[derive(Default)]
struct LockState {
    key: Option<vault::VaultKey>,
    salt: Vec<u8>,
}

/// Felt on every wrong app password, on top of what Argon2 costs.
const WRONG_PASSWORD_PAUSE: Duration = Duration::from_secs(if cfg!(test) { 0 } else { 1 });

fn now_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

/// Decryption is CPU work: on the async runtime it would stall every other request.
async fn on_worker<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(work).await.map_err(|e| Error::other(e.to_string()))?
}

impl Service {
    /// `on_sign_out` runs whenever the session ends, and says whether it was by choice.
    /// Reaches Proton directly from the start: for tests and for programs borrowing the app's session.
    pub fn new(
        base: &str,
        store: Arc<dyn SessionStore>,
        on_sign_out: Box<dyn Fn(Ending) + Send + Sync>,
    ) -> Result<Arc<Self>> {
        let service = Self::undecided(base, store, on_sign_out);
        service.choose_way(&crate::client::Way::Direct)?;
        Ok(service)
    }

    /// With no way to Proton yet: nothing goes out until `choose_way`, which the app calls once it knows whether the
    /// person wants Tor.
    pub fn undecided(
        base: &str,
        store: Arc<dyn SessionStore>,
        on_sign_out: Box<dyn Fn(Ending) + Send + Sync>,
    ) -> Arc<Self> {
        let client = Arc::new(Client::undecided(base));
        let service = Arc::new_cyclic(|me| Self {
            me: me.clone(),
            client,
            crypto: engine(),
            store,
            key_secret: Arc::default(),
            addresses: RwLock::default(),
            login: Mutex::default(),
            sender_keys: Mutex::default(),
            mime_files: Mutex::default(),
            opened: Mutex::default(),
            threads: Mutex::default(),
            logos: Mutex::default(),
            staged: Mutex::default(),
            last_counts: Mutex::default(),
            lock: Mutex::default(),
            attempts: Mutex::default(),
            password_gate: tokio::sync::Mutex::new(()),
            view: Mutex::default(),
            view_listener: Mutex::default(),
            stage_listener: Mutex::default(),
            mail_dir: Mutex::default(),
            mail_store: RwLock::default(),
            store_key: Mutex::default(),
            opening_ahead: AtomicBool::new(false),
            next_on_move: AtomicBool::new(false),
            habits: RwLock::new(Habits::default()),
        });
        let weak = Arc::downgrade(&service);
        service.client.on_session_change(Box::new(move |change| {
            let Some(service) = weak.upgrade() else { return };
            match change {
                SessionChange::Renewed(session) => service.persist(session),
                SessionChange::Ended(ending) => {
                    service.forget_everything();
                    on_sign_out(ending);
                }
            }
        }));
        service
    }

    /// How Proton is reached, chosen once for the life of the app. See `client::Way`.
    pub fn choose_way(&self, way: &crate::client::Way) -> Result<()> {
        self.client.decide(way)
    }

    /// Whether everything goes through Tor.
    #[must_use]
    pub fn through_tor(&self) -> bool {
        self.client.through_tor()
    }

    /// However the session ended, by choice or because it was revoked from elsewhere, nothing of
    /// the account stays behind: not on disk, and not in this process.
    fn forget_everything(&self) {
        self.wipe_memory();
        if let Some(dir) = self.mail_dir.lock().as_ref() {
            mail_store::erase(dir);
        }
        *self.lock.lock() = LockState::default();
        self.store.clear();
        self.reset_attempts();
        self.cancel_login();
    }

    /// Everything of the account held by this process: secrets are overwritten, mail is released.
    fn wipe_memory(&self) {
        *self.view.lock() = crate::view::View::default();
        *self.key_secret.lock() = None;
        *self.mail_store.write() = None;
        *self.store_key.lock() = None;
        self.crypto.forget_keys();
        self.addresses.write().clear();
        self.sender_keys.lock().clear();
        self.mime_files.lock().clear();
        self.opened.lock().clear();
        self.threads.lock().clear();
        self.logos.lock().clear();
        self.staged.lock().clear();
        *self.last_counts.lock() = None;
    }

    fn persist(&self, session: &Session) {
        let secret = self.key_secret.lock().as_ref().map(|s| Zeroizing::new(STANDARD.encode(s.as_slice())));
        let Some(secret) = secret else {
            return;
        };
        let store_key = self.store_key.lock().as_ref().map(|key| Zeroizing::new(STANDARD.encode(key.as_slice())));
        let saved = SavedSession { session: session.clone(), key_secret: secret, store_key };
        let Ok(plain) = serde_json::to_string(&saved).map(Zeroizing::new) else { return };
        let lock = self.lock.lock();
        let value = match &lock.key {
            Some(key) => vault::seal(key, &lock.salt, plain.as_bytes())
                .ok()
                .and_then(|envelope| serde_json::to_string(&envelope).ok())
                .map(Zeroizing::new),
            None => Some(plain),
        };
        drop(lock);
        if let Some(value) = value {
            if value.encode_utf16().count() * 2 > crate::WINDOWS_CREDENTIAL_BYTES {
                log::warn!("the saved session is longer than Windows keeps: {} characters", value.len());
            }
            self.store.save(&value);
        }
    }
}

fn text(value: &Value, name: &str) -> String {
    value.get(name).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn number(value: &Value, name: &str) -> u64 {
    value.get(name).and_then(Value::as_u64).unwrap_or(0)
}

pub use print::{PrintCheck, PrintScope, SealCheck, SheetReport};
pub use sensitive::{Confirmation, TotpSetup};

#[cfg(test)]
mod tests;
