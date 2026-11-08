//! All OpenPGP work, on Proton's own Rust crates. Key objects never leave this file: callers name
//! an address and pass bytes, which also keeps private keys out of every other module.

use crate::error::{Error, Result};
use parking_lot::RwLock;
use proton_crypto_account::{
    keys::{AddressKeys, LockedKey, UnlockedAddressKey, UnlockedUserKey, UserKeys},
    proton_crypto::{
        crypto::{
            AccessKeyInfo, DataEncoding, Verifier, VerifierSync, Decryptor, DecryptorSync, Encryptor, EncryptorSync, PGPMessage,
            PGPProviderSync, SessionKeyAlgorithm, Signer, SignerSync, UnixTimestamp,
            VerificationError, VerifiedData,
        },
        new_pgp_provider, new_srp_provider,
        srp::SRPProvider,
    },
    salts::{KeySecret, Salt, Salts},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

/// A sender's clock may run ahead of ours, and that must not make a good signature look forged.
const CLOCK_SKEW_SECONDS: u64 = 24 * 3600;
const AES256: &str = "aes256";
const AES128: &str = "aes128";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureStatus {
    Valid,
    Invalid,
    Unknown,
    Unsigned,
}

pub struct Decrypted {
    pub data: Vec<u8>,
    pub signature: SignatureStatus,
}

#[derive(Clone)]
pub struct RawSessionKey {
    pub key: Vec<u8>,
    pub algorithm: &'static str,
}

pub struct BodyMaterial {
    /// What the draft stores: encrypted to our own key.
    pub armored: String,
    /// The body alone, shared by every recipient's package.
    pub data_packet: Vec<u8>,
    pub session_key: RawSessionKey,
}

pub struct AttachmentMaterial {
    pub session_key: RawSessionKey,
    pub data_packet: Vec<u8>,
    /// The session key wrapped to the sender, for the upload.
    pub key_packets: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct KeySaltRow {
    #[serde(rename = "ID")]
    pub id: String,
    pub key_salt: Option<String>,
}

pub struct AddressInput {
    pub id: String,
    pub keys: Vec<LockedKey>,
}

/// Enough to tell a key by. The address is `None` for the account's own key.
pub struct KeyFingerprint {
    pub address_id: Option<String>,
    pub fingerprint: String,
    pub primary: bool,
}

/// The operations the rest of the program may ask for. `Service` holds this as a trait object,
/// because Proton's provider type cannot be named.
pub trait Crypto: Send + Sync {
    /// The bytes that open the user keys. Stored in the keychain: they read mail, but cannot log in.
    fn key_secret(&self, password: &str, keys: &[LockedKey], salts: &[KeySaltRow]) -> Result<Vec<u8>>;
    fn unlock(&self, user_keys: Vec<LockedKey>, addresses: Vec<AddressInput>, secret: &[u8]) -> Result<()>;
    fn forget_keys(&self);
    fn has_keys_for(&self, address_id: &str) -> bool;
    fn key_fingerprints(&self) -> Vec<KeyFingerprint>;
    fn decrypt_message(&self, armored: &str, address_id: &str, sender_keys: &[String]) -> Result<Decrypted>;
    fn decrypt_attachment(&self, key_packets: &[u8], data: &[u8], address_id: &str) -> Result<Vec<u8>>;
    fn session_key(&self, key_packets: &[u8], address_id: &str) -> Result<RawSessionKey>;
    fn encrypt_body(&self, text: &str, address_id: &str, signed_at: Option<u64>) -> Result<BodyMaterial>;
    fn encrypt_attachment(&self, data: &[u8], address_id: &str) -> Result<AttachmentMaterial>;
    fn wrap_for_key(&self, session_key: &RawSessionKey, armored_public_key: &str) -> Result<Vec<u8>>;
    fn wrap_for_password(&self, session_key: &RawSessionKey, password: &str) -> Result<Vec<u8>>;
    fn encrypt_for_password(&self, text: &str, password: &str) -> Result<String>;
    fn sign_with_user_key(&self, text: &str) -> Result<String>;
    /// A standard OpenPGP clear-signed message, by this address's primary key.
    fn clearsign(&self, text: &str, address_id: &str) -> Result<String>;
    /// What a clear-signed message says, if one of these keys signed it.
    fn clearsigned_text(&self, armored: &str, armored_keys: &[String]) -> Option<String>;
    fn login_proof(&self, username: &str, password: &str, info: &AuthInfo) -> Result<LoginProof>;
    /// `(salt, verifier)` for a new password: the server can check it but never learns it.
    fn password_verifier(&self, password: &str, signed_modulus: &str) -> Result<(String, String)>;
}

pub struct AuthInfo {
    pub version: u8,
    pub modulus: String,
    pub server_ephemeral: String,
    pub salt: String,
}

pub struct LoginProof {
    pub ephemeral: String,
    pub proof: String,
    pub expected_server_proof: String,
}

fn seconds_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

const fn algorithm_name(algorithm: SessionKeyAlgorithm) -> &'static str {
    match algorithm {
        SessionKeyAlgorithm::Aes128 => AES128,
        _ => AES256,
    }
}

fn algorithm_of(name: &str) -> SessionKeyAlgorithm {
    if name == AES128 {
        SessionKeyAlgorithm::Aes128
    } else {
        SessionKeyAlgorithm::Aes256
    }
}

const fn status_of(result: &std::result::Result<(), VerificationError>) -> SignatureStatus {
    match result {
        Ok(()) => SignatureStatus::Valid,
        Err(VerificationError::NotSigned(_)) => SignatureStatus::Unsigned,
        Err(VerificationError::Failed(..) | VerificationError::BadContext(..)) => SignatureStatus::Invalid,
        Err(_) => SignatureStatus::Unknown,
    }
}

struct AddressKeyring<P: PGPProviderSync> {
    id: String,
    keys: Vec<UnlockedAddressKey<P>>,
}

struct Keyring<P: PGPProviderSync> {
    users: Vec<UnlockedUserKey<P>>,
    addresses: Vec<AddressKeyring<P>>,
}

impl<P: PGPProviderSync> Default for Keyring<P> {
    fn default() -> Self {
        Self { users: Vec::new(), addresses: Vec::new() }
    }
}

impl<P: PGPProviderSync> Keyring<P> {
    /// The message's own address first, then everything else: mail may have been moved between
    /// addresses, and older messages are encrypted to user keys.
    fn decryption_keys<'a>(&'a self, address_id: &str) -> Vec<&'a P::PrivateKey> {
        let own = self.addresses.iter().filter(|a| a.id == address_id);
        let others = self.addresses.iter().filter(|a| a.id != address_id);
        own.chain(others)
            .flat_map(|address| address.keys.iter().map(|k| &k.private_key))
            .chain(self.users.iter().map(|k| &k.private_key))
            .collect()
    }

    fn signing_key(&self, address_id: &str) -> Result<&UnlockedAddressKey<P>> {
        let address = self.addresses.iter().find(|a| a.id == address_id);
        let keys = address.map_or(&[][..], |a| &a.keys);
        keys.iter()
            .find(|k| k.primary)
            .or_else(|| keys.first())
            .ok_or_else(|| Error::other("This address has no usable key"))
    }
}

struct Engine<P: PGPProviderSync, S: SRPProvider> {
    pgp: P,
    srp: S,
    keyring: RwLock<Keyring<P>>,
}

impl<P: PGPProviderSync, S: SRPProvider> Engine<P, S> {
    fn import_session_key(&self, raw: &RawSessionKey) -> Result<P::SessionKey> {
        self.pgp.session_key_import(&raw.key, algorithm_of(raw.algorithm)).map_err(Error::crypto)
    }

    fn export_session_key(&self, key: &P::SessionKey) -> Result<RawSessionKey> {
        let (bytes, algorithm) = self.pgp.session_key_export(key).map_err(Error::crypto)?;
        Ok(RawSessionKey { key: bytes.as_ref().to_vec(), algorithm: algorithm_name(algorithm) })
    }

    fn generate_session_key(&self) -> Result<P::SessionKey> {
        self.pgp.session_key_generate(SessionKeyAlgorithm::Aes256).map_err(Error::crypto)
    }
}

impl<P: PGPProviderSync, S: SRPProvider + 'static> Crypto for Engine<P, S> {
    fn key_secret(&self, password: &str, keys: &[LockedKey], salts: &[KeySaltRow]) -> Result<Vec<u8>> {
        let primary = keys.iter().find(|k| k.primary).or_else(|| keys.first());
        let id = primary.ok_or_else(|| Error::other("This account has no keys"))?.id.clone();
        let salted = salts.iter().find(|s| s.id == id.0).and_then(|s| s.key_salt.clone());
        // Accounts from old auth versions have no salt: their passphrase is the password itself.
        let Some(salt) = salted else {
            return Ok(password.as_bytes().to_vec());
        };
        let table = Salts::new(vec![Salt { id: id.clone(), key_salt: Some(salt.into()) }]);
        let secret = table.salt_for_key(&self.srp, &id, password.as_bytes()).map_err(Error::crypto)?;
        Ok(secret.as_ref().to_vec())
    }

    fn unlock(&self, user_keys: Vec<LockedKey>, addresses: Vec<AddressInput>, secret: &[u8]) -> Result<()> {
        let secret = KeySecret::new(secret.to_vec());
        let active = |keys: Vec<LockedKey>| {
            let mut keys: Vec<_> = keys.into_iter().filter(|k| k.active).collect();
            keys.sort_by_key(|k| !k.primary);
            keys
        };
        let users = UserKeys::new(active(user_keys)).unlock(&self.pgp, &secret).unlocked_keys;
        if users.is_empty() {
            return Err(Error::other("Could not unlock any user key: wrong mailbox password?"));
        }
        let addresses = addresses
            .into_iter()
            .map(|address| AddressKeyring {
                keys: AddressKeys::new(active(address.keys))
                    .unlock(&self.pgp, &users, Some(&secret))
                    .unlocked_keys,
                id: address.id,
            })
            .collect();
        *self.keyring.write() = Keyring { users, addresses };
        Ok(())
    }

    fn forget_keys(&self) {
        *self.keyring.write() = Keyring::default();
    }

    #[expect(clippy::significant_drop_tightening, reason = "the keys are borrowed from the guard until the end")]
    fn key_fingerprints(&self) -> Vec<KeyFingerprint> {
        let keyring = self.keyring.read();
        let users = keyring.users.iter().enumerate().map(|(index, key)| KeyFingerprint {
            address_id: None,
            fingerprint: key.private_key.key_fingerprint().to_string(),
            primary: index == 0,
        });
        let addresses = keyring.addresses.iter().flat_map(|address| {
            address.keys.iter().map(|key| KeyFingerprint {
                address_id: Some(address.id.clone()),
                fingerprint: key.private_key.key_fingerprint().to_string(),
                primary: key.primary,
            })
        });
        users.chain(addresses).collect()
    }

    fn has_keys_for(&self, address_id: &str) -> bool {
        self.keyring.read().addresses.iter().any(|a| a.id == address_id && !a.keys.is_empty())
    }

    fn decrypt_message(&self, armored: &str, address_id: &str, sender_keys: &[String]) -> Result<Decrypted> {
        let senders: Vec<P::PublicKey> = sender_keys
            .iter()
            .filter_map(|key| self.pgp.public_key_import(key, DataEncoding::Armor).ok())
            .collect();
        let keyring = self.keyring.read();
        let verified = self
            .pgp
            .new_decryptor()
            .with_decryption_keys(keyring.decryption_keys(address_id))
            .with_verification_keys(&senders)
            .at_verification_time(UnixTimestamp::new(seconds_now() + CLOCK_SKEW_SECONDS))
            .decrypt(armored, DataEncoding::Armor)
            .map_err(Error::crypto)?;
        drop(keyring);
        let signature = status_of(&verified.verification_result().map(drop));
        Ok(Decrypted { data: verified.into_vec(), signature })
    }

    fn decrypt_attachment(&self, key_packets: &[u8], data: &[u8], address_id: &str) -> Result<Vec<u8>> {
        let keyring = self.keyring.read();
        let session_key = self
            .pgp
            .new_decryptor()
            .with_decryption_keys(keyring.decryption_keys(address_id))
            .decrypt_session_key(key_packets)
            .map_err(Error::crypto)?;
        drop(keyring);
        let opened = self
            .pgp
            .new_decryptor()
            .with_session_key(session_key)
            .decrypt(data, DataEncoding::Bytes)
            .map_err(Error::crypto)?;
        Ok(opened.into_vec())
    }

    fn session_key(&self, key_packets: &[u8], address_id: &str) -> Result<RawSessionKey> {
        let keyring = self.keyring.read();
        let key = self
            .pgp
            .new_decryptor()
            .with_decryption_keys(keyring.decryption_keys(address_id))
            .decrypt_session_key(key_packets)
            .map_err(Error::crypto)?;
        drop(keyring);
        self.export_session_key(&key)
    }

    /// Signing is mandatory: for recipients outside Proton the server opens the body to deliver
    /// ordinary mail and refuses unsigned ones. A scheduled send is dated for its delivery time.
    #[expect(clippy::significant_drop_tightening, reason = "the keys are borrowed from the guard until the end")]
    fn encrypt_body(&self, text: &str, address_id: &str, signed_at: Option<u64>) -> Result<BodyMaterial> {
        let keyring = self.keyring.read();
        let signer = keyring.signing_key(address_id)?;
        let session_key = self.generate_session_key()?;
        let mut encryptor = self
            .pgp
            .new_encryptor()
            .with_encryption_key(&signer.public_key)
            .with_signing_key(&signer.private_key)
            .with_session_key_ref(&session_key)
            .with_utf8();
        if let Some(seconds) = signed_at {
            encryptor = encryptor.at_signing_time(UnixTimestamp::new(seconds));
        }
        let message = encryptor.encrypt(text).map_err(Error::crypto)?;
        let armored = String::from_utf8(message.armor().map_err(Error::crypto)?)
            .map_err(Error::crypto)?;
        Ok(BodyMaterial {
            armored,
            data_packet: message.as_data_packet().to_vec(),
            session_key: self.export_session_key(&session_key)?,
        })
    }

    #[expect(clippy::significant_drop_tightening, reason = "the keys are borrowed from the guard until the end")]
    fn encrypt_attachment(&self, data: &[u8], address_id: &str) -> Result<AttachmentMaterial> {
        let keyring = self.keyring.read();
        let signer = keyring.signing_key(address_id)?;
        let session_key = self.generate_session_key()?;
        let message = self
            .pgp
            .new_encryptor()
            .with_encryption_key(&signer.public_key)
            .with_session_key_ref(&session_key)
            .encrypt(data)
            .map_err(Error::crypto)?;
        let signature = self
            .pgp
            .new_signer()
            .with_signing_key(&signer.private_key)
            .sign_detached(data, DataEncoding::Bytes)
            .map_err(Error::crypto)?;
        Ok(AttachmentMaterial {
            session_key: self.export_session_key(&session_key)?,
            data_packet: message.as_data_packet().to_vec(),
            key_packets: message.as_key_packets().to_vec(),
            signature,
        })
    }

    fn wrap_for_key(&self, session_key: &RawSessionKey, armored_public_key: &str) -> Result<Vec<u8>> {
        let key = self.import_session_key(session_key)?;
        let recipient =
            self.pgp.public_key_import(armored_public_key, DataEncoding::Armor).map_err(Error::crypto)?;
        let packets = self
            .pgp
            .new_encryptor()
            .with_encryption_key(&recipient)
            .encrypt_session_key(&key)
            .map_err(Error::crypto)?;
        Ok(packets)
    }

    fn wrap_for_password(&self, session_key: &RawSessionKey, password: &str) -> Result<Vec<u8>> {
        let key = self.import_session_key(session_key)?;
        let packets = self
            .pgp
            .new_encryptor()
            .with_passphrase(password)
            .encrypt_session_key(&key)
            .map_err(Error::crypto)?;
        Ok(packets)
    }

    fn encrypt_for_password(&self, text: &str, password: &str) -> Result<String> {
        let message = self
            .pgp
            .new_encryptor()
            .with_passphrase(password)
            .with_utf8()
            .encrypt_raw(text, DataEncoding::Armor)
            .map_err(Error::crypto)?;
        String::from_utf8(message).map_err(Error::crypto)
    }

    #[expect(clippy::significant_drop_tightening, reason = "the keys are borrowed from the guard until the end")]
    fn clearsign(&self, text: &str, address_id: &str) -> Result<String> {
        let keyring = self.keyring.read();
        let key = keyring.signing_key(address_id)?;
        let signed = self
            .pgp
            .new_signer()
            .with_signing_key(&key.private_key)
            .with_utf8()
            .sign_cleartext(text)
            .map_err(Error::crypto)?;
        String::from_utf8(signed).map_err(Error::crypto)
    }

    fn clearsigned_text(&self, armored: &str, armored_keys: &[String]) -> Option<String> {
        let keys: Vec<P::PublicKey> = armored_keys
            .iter()
            .filter_map(|key| self.pgp.public_key_import(key, DataEncoding::Armor).ok())
            .collect();
        if keys.is_empty() {
            return None;
        }
        let verified = self.pgp.new_verifier().with_verification_keys(&keys).verify_cleartext(armored).ok()?;
        verified.verification_result().ok()?;
        String::from_utf8(verified.as_ref().to_vec()).ok()
    }

    #[expect(clippy::significant_drop_tightening, reason = "the keys are borrowed from the guard until the end")]
    fn sign_with_user_key(&self, text: &str) -> Result<String> {
        let keyring = self.keyring.read();
        let key = keyring.users.first().ok_or_else(|| Error::other("The keys are locked"))?;
        let signature = self
            .pgp
            .new_signer()
            .with_signing_key(&key.private_key)
            .with_utf8()
            .sign_detached(text, DataEncoding::Armor)
            .map_err(Error::crypto)?;
        String::from_utf8(signature).map_err(Error::crypto)
    }

    fn password_verifier(&self, password: &str, signed_modulus: &str) -> Result<(String, String)> {
        let verifier =
            self.srp.generate_client_verifier(password, signed_modulus).map_err(Error::crypto)?;
        Ok((verifier.salt, verifier.verifier))
    }

    fn login_proof(&self, username: &str, password: &str, info: &AuthInfo) -> Result<LoginProof> {
        let proof = self
            .srp
            .generate_client_proof(
                username,
                password,
                info.version,
                &info.salt,
                &info.modulus,
                &info.server_ephemeral,
            )
            .map_err(Error::crypto)?;
        Ok(LoginProof {
            ephemeral: proof.ephemeral,
            proof: proof.proof,
            expected_server_proof: proof.expected_server_proof,
        })
    }
}

#[must_use]
pub fn engine() -> Arc<dyn Crypto> {
    Arc::new(Engine { pgp: new_pgp_provider(), srp: new_srp_provider(), keyring: RwLock::default() })
}

#[cfg(test)]
mod tests;
