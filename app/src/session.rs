//! The session lives in the system keychain, never in a file on disk.

use keyring::{Entry, Error};
use proton_core::{
    SessionStore,
    journal::{self, JournalKey, KEY_BYTES},
    vault,
};
use std::sync::Arc;
use zeroize::Zeroizing;

const SERVICE: &str = "org.koval-dev.proton-mail-light";
const ACCOUNT: &str = "session";
const ATTEMPTS: &str = "unlock-attempts";
const UNLOCK_KEY: &str = "unlock-key";
/// An entry of its own: the system would stop the start to ask about a key another build made, and every other
/// keychain call would wait behind that question.
const JOURNAL_KEY: &str = "journal-key-native";

struct Keychain;

/// The name of a keychain error, safe for the journal: its text can name the entry.
const fn kind(error: &Error) -> &'static str {
    match error {
        Error::NoEntry => "no entry",
        Error::TooLong(..) => "too long for the system",
        Error::NoStorageAccess(_) => "no access to the store",
        Error::PlatformFailure(_) => "the store failed",
        Error::BadEncoding(_) => "bad encoding",
        Error::Ambiguous(_) => "ambiguous",
        Error::Invalid(..) => "invalid",
        _ => "other",
    }
}

fn entry_for(account: &str) -> Option<Entry> {
    Entry::new(SERVICE, account).ok()
}

impl SessionStore for Keychain {
    fn load(&self) -> Option<String> {
        entry_for(ACCOUNT)?.get_password().ok()
    }

    fn save(&self, value: &str) {
        let saved = entry_for(ACCOUNT).map(|entry| entry.set_password(value));
        if let Some(Err(error)) = saved {
            log::error!("the session could not be kept: {}", kind(&error));
        }
    }

    fn clear(&self) {
        if let Some(entry) = entry_for(ACCOUNT) {
            let _ = entry.delete_credential();
        }
    }

    fn load_attempts(&self) -> Option<String> {
        entry_for(ATTEMPTS)?.get_password().ok()
    }

    fn save_attempts(&self, value: &str) {
        let saved = entry_for(ATTEMPTS).map(|entry| entry.set_password(value));
        if let Some(Err(error)) = saved {
            log::error!("wrong app passwords could not be counted: {}", kind(&error));
        }
    }

    fn clear_attempts(&self) {
        if let Some(entry) = entry_for(ATTEMPTS) {
            let _ = entry.delete_credential();
        }
    }
}

/// Apart from the session: signing out must not cost the record of what went wrong.
pub fn journal_key() -> Option<JournalKey> {
    let entry = entry_for(JOURNAL_KEY)?;
    match entry.get_secret() {
        Ok(stored) => {
            let stored = Zeroizing::new(stored);
            let bytes: [u8; KEY_BYTES] = stored.as_slice().try_into().ok()?;
            Some(Zeroizing::new(bytes))
        }
        Err(Error::NoEntry) => {
            let key = journal::new_key();
            entry.set_secret(key.as_slice()).ok()?;
            Some(key)
        }
        Err(_) => None,
    }
}

pub fn keychain() -> Arc<dyn SessionStore> {
    Arc::new(Keychain)
}

pub fn forget_unlock_key() {
    if let Some(entry) = entry_for(UNLOCK_KEY) {
        let _ = entry.delete_credential();
    }
}

pub fn store_unlock_key(key: &[u8]) -> bool {
    entry_for(UNLOCK_KEY).is_some_and(|entry| entry.set_secret(key).is_ok())
}

pub fn has_unlock_key() -> bool {
    entry_for(UNLOCK_KEY).is_some_and(|entry| !matches!(entry.get_password(), Err(Error::NoEntry)))
}

/// The key Touch ID hands back, once the system has confirmed the person.
pub fn unlock_key() -> Option<vault::VaultKey> {
    let stored = Zeroizing::new(entry_for(UNLOCK_KEY)?.get_secret().ok()?);
    let key: [u8; vault::KEY_BYTES] = stored.as_slice().try_into().ok()?;
    Some(Zeroizing::new(key))
}

#[cfg(target_os = "macos")]
pub mod touch_id {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};
    use std::sync::mpsc;

    const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthenticationWithBiometrics;

    pub fn available() -> bool {
        // Safety: a fresh context, queried on this thread.
        unsafe { LAContext::new().canEvaluatePolicy_error(POLICY).is_ok() }
    }

    /// Waits for the system's own prompt: the fingerprint never reaches this program.
    pub fn confirm(reason: &str) -> bool {
        let (sender, answer) = mpsc::channel();
        let reply = RcBlock::new(move |accepted: Bool, _: *mut NSError| {
            let _ = sender.send(accepted.as_bool());
        });
        // Safety: the block lives until the call returns, and the system copies it.
        unsafe {
            let context = LAContext::new();
            context.evaluatePolicy_localizedReason_reply(POLICY, &NSString::from_str(reason), &reply);
        }
        answer.recv().unwrap_or(false)
    }
}

#[cfg(not(target_os = "macos"))]
pub mod touch_id {
    pub const fn available() -> bool {
        false
    }

    pub const fn confirm(_: &str) -> bool {
        false
    }
}
