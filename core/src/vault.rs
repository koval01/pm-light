//! Seals the saved session under the app password. Without that password the keychain holds only
//! noise, so reaching the logged-in user's keychain is no longer enough to reach their mail.

use crate::error::{Error, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    Key, XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
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
    rand::random()
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
    let nonce: [u8; NONCE_BYTES] = rand::random();
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
mod tests;
