//! Seals the saved session under the app password. Without that password the keychain holds only
//! noise, so reaching the logged-in user's keychain is no longer enough to reach their mail.

use crate::error::{Error, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const KEY_BYTES: usize = 32;
pub const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 24;
/// OWASP's first Argon2id recommendation: slow for a guesser, a blink for the person.
const MEMORY_KIB: u32 = if cfg!(test) { 256 } else { 64 * 1024 };
const PASSES: u32 = if cfg!(test) { 1 } else { 3 };

/// Wrong passwords that cost only the pause, before the app starts to freeze.
const FREE_ATTEMPTS: u32 = 4;
/// Each further wrong password, in seconds. The last one repeats.
const FREEZES: [u64; 5] = [30, 60, 300, 900, 3600];
const LONGEST_FREEZE: u64 = 3600;

pub type VaultKey = Zeroizing<[u8; KEY_BYTES]>;

/// Wrong app passwords in a row. Kept apart from the session, so that a restart does not reset them.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempts {
    failures: u32,
    frozen_until: u64,
}

impl Attempts {
    /// Seconds until the next try. A clock set back can never make this longer than the longest freeze.
    #[must_use]
    pub fn wait(&self, now: u64) -> u64 {
        self.frozen_until.saturating_sub(now).min(LONGEST_FREEZE)
    }

    pub fn failed(&mut self, now: u64) {
        self.failures = self.failures.saturating_add(1);
        if let Some(past_free) = self.failures.checked_sub(FREE_ATTEMPTS + 1) {
            let step = usize::try_from(past_free).unwrap_or(usize::MAX);
            self.frozen_until = now.saturating_add(FREEZES.get(step).copied().unwrap_or(LONGEST_FREEZE));
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Envelope {
    pub salt: String,
    pub nonce: String,
    pub sealed: String,
}

#[must_use]
pub fn new_salt() -> [u8; SALT_BYTES] {
    let mut salt = [0; SALT_BYTES];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

pub fn derive_key(password: &str, salt: &[u8]) -> Result<VaultKey> {
    let params = Params::new(MEMORY_KIB, PASSES, 1, Some(KEY_BYTES)).map_err(Error::crypto)?;
    let mut key = Zeroizing::new([0; KEY_BYTES]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(Error::crypto)?;
    Ok(key)
}

pub fn seal(key: &[u8; KEY_BYTES], salt: &[u8], plain: &[u8]) -> Result<Envelope> {
    let mut nonce = [0; NONCE_BYTES];
    rand::thread_rng().fill_bytes(&mut nonce);
    let sealed = XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(XNonce::from_slice(&nonce), plain)
        .map_err(Error::crypto)?;
    Ok(Envelope {
        salt: STANDARD.encode(salt),
        nonce: STANDARD.encode(nonce),
        sealed: STANDARD.encode(sealed),
    })
}

/// A wrong key and a tampered envelope look the same, and neither reveals anything.
pub fn open(key: &[u8; KEY_BYTES], envelope: &Envelope) -> Result<Zeroizing<Vec<u8>>> {
    let nonce = STANDARD.decode(&envelope.nonce).map_err(|_| Error::WrongPassword)?;
    let sealed = STANDARD.decode(&envelope.sealed).map_err(|_| Error::WrongPassword)?;
    if nonce.len() != NONCE_BYTES {
        return Err(Error::WrongPassword);
    }
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(XNonce::from_slice(&nonce), sealed.as_slice())
        .map(Zeroizing::new)
        .map_err(|_| Error::WrongPassword)
}

pub fn salt_of(envelope: &Envelope) -> Result<Vec<u8>> {
    STANDARD.decode(&envelope.salt).map_err(|_| Error::WrongPassword)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_freeze_is_the_last_step() {
        assert_eq!(FREEZES.last(), Some(&LONGEST_FREEZE));
    }

    fn sealed(password: &str) -> (Envelope, VaultKey) {
        let salt = new_salt();
        let key = derive_key(password, &salt).unwrap();
        (seal(&key, &salt, b"session").unwrap(), key)
    }

    #[test]
    fn the_first_wrong_passwords_do_not_freeze() {
        let mut attempts = Attempts::default();
        for _ in 0..FREE_ATTEMPTS {
            attempts.failed(1000);
        }
        assert_eq!(attempts.wait(1000), 0);
    }

    #[test]
    fn each_further_wrong_password_freezes_longer_up_to_a_limit() {
        let mut attempts = Attempts::default();
        for _ in 0..FREE_ATTEMPTS {
            attempts.failed(1000);
        }
        let waits: Vec<u64> = (0..7)
            .map(|_| {
                attempts.failed(1000);
                attempts.wait(1000)
            })
            .collect();
        assert_eq!(waits, [30, 60, 300, 900, 3600, 3600, 3600]);
    }

    #[test]
    fn a_freeze_runs_out() {
        let mut attempts = Attempts::default();
        for _ in 0..=FREE_ATTEMPTS {
            attempts.failed(1000);
        }
        assert_eq!(attempts.wait(1029), 1);
        assert_eq!(attempts.wait(1030), 0);
    }

    #[test]
    fn a_clock_set_back_cannot_freeze_for_longer_than_the_limit() {
        let attempts = Attempts { failures: 9, frozen_until: 10_000_000 };
        assert_eq!(attempts.wait(0), 3600);
    }

    #[test]
    fn the_right_password_opens_it() {
        let (envelope, _) = sealed("café password");
        let key = derive_key("café password", &salt_of(&envelope).unwrap()).unwrap();
        assert_eq!(open(&key, &envelope).unwrap().as_slice(), b"session");
    }

    #[test]
    fn a_wrong_password_opens_nothing() {
        let (envelope, _) = sealed("right");
        let key = derive_key("wrong", &salt_of(&envelope).unwrap()).unwrap();
        assert!(matches!(open(&key, &envelope), Err(Error::WrongPassword)));
    }

    #[test]
    fn a_tampered_envelope_is_refused() {
        let (mut envelope, key) = sealed("right");
        let mut bytes = STANDARD.decode(&envelope.sealed).unwrap();
        bytes[0] ^= 1;
        envelope.sealed = STANDARD.encode(bytes);
        assert!(matches!(open(&key, &envelope), Err(Error::WrongPassword)));
    }

    #[test]
    fn junk_in_the_envelope_is_refused_not_panicked_on() {
        let (mut envelope, key) = sealed("right");
        envelope.nonce = "!!!".into();
        assert!(open(&key, &envelope).is_err());
        envelope.nonce = STANDARD.encode([0u8; 3]);
        assert!(open(&key, &envelope).is_err());
    }

    #[test]
    fn the_same_data_never_seals_the_same_way() {
        let salt = new_salt();
        let key = derive_key("x", &salt).unwrap();
        assert_ne!(seal(&key, &salt, b"a").unwrap().sealed, seal(&key, &salt, b"a").unwrap().sealed);
    }

    #[test]
    fn the_session_is_not_readable_in_the_envelope() {
        let (envelope, _) = sealed("right");
        assert!(!serde_json::to_string(&envelope).unwrap().contains("session\""));
        assert!(!String::from_utf8_lossy(&STANDARD.decode(envelope.sealed).unwrap()).contains("session"));
    }
}
