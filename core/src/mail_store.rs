//! Mail kept on disk, so that a message read once opens at once, even after a restart.
//!
//! Each file is sealed on its own, its name says nothing of what is inside, and the key travels with the saved
//! session: an app password protects this as it protects the session, and signing out ends it.

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use rand::RngCore;
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use zeroize::Zeroizing;

pub const KEY_BYTES: usize = 32;
pub type StoreKey = Zeroizing<[u8; KEY_BYTES]>;

const NONCE_BYTES: usize = 24;
const EXTENSION: &str = "sealed";
const MOST_BYTES: u64 = 256 * 1024 * 1024;
const MOST_AGE: Duration = Duration::from_hours(720);

#[must_use]
pub fn new_key() -> StoreKey {
    let mut key = Zeroizing::new([0; KEY_BYTES]);
    rand::thread_rng().fill_bytes(key.as_mut());
    key
}

pub struct MailStore {
    dir: PathBuf,
    cipher: XChaCha20Poly1305,
    key: StoreKey,
}

impl MailStore {
    #[must_use]
    pub fn open(dir: &Path, key: StoreKey) -> Option<Self> {
        fs::create_dir_all(dir).ok()?;
        #[cfg(unix)]
        fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700)).ok()?;
        let cipher = XChaCha20Poly1305::new_from_slice(key.as_slice()).ok()?;
        Some(Self { dir: dir.to_path_buf(), cipher, key })
    }

    /// Ids are the account's business too, so names come from the key and reveal nothing.
    fn name(&self, kind: &str, id: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(self.key.as_slice());
        hash.update(kind.as_bytes());
        hash.update([0]);
        hash.update(id.as_bytes());
        crate::hex(&hash.finalize())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.{EXTENSION}"))
    }

    /// A failed write costs a later wait and nothing else.
    pub fn put<T: Serialize>(&self, kind: &str, id: &str, value: &T) {
        let name = self.name(kind, id);
        let Ok(plain) = serde_json::to_vec(value).map(Zeroizing::new) else { return };
        let mut nonce = [0; NONCE_BYTES];
        rand::thread_rng().fill_bytes(&mut nonce);
        let payload = Payload { msg: &plain, aad: name.as_bytes() };
        let Ok(sealed) = self.cipher.encrypt(XNonce::from_slice(&nonce), payload) else { return };
        let path = self.path(&name);
        let partial = path.with_extension("partial");
        let mut options = OpenOptions::new();
        options.create(true).write(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let Ok(mut file) = options.open(&partial) else { return };
        if file.write_all(&nonce).and_then(|()| file.write_all(&sealed)).is_ok() {
            let _ = fs::rename(&partial, &path);
        } else {
            let _ = fs::remove_file(&partial);
        }
    }

    /// Anything that does not open is treated as never written, and removed.
    #[must_use]
    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Option<T> {
        let name = self.name(kind, id);
        let path = self.path(&name);
        let bytes = fs::read(&path).ok()?;
        let opened = bytes.split_at_checked(NONCE_BYTES).and_then(|(nonce, sealed)| {
            let payload = Payload { msg: sealed, aad: name.as_bytes() };
            let plain = self.cipher.decrypt(XNonce::from_slice(nonce), payload).ok()?;
            serde_json::from_slice(&Zeroizing::new(plain)).ok()
        });
        if opened.is_none() {
            let _ = fs::remove_file(&path);
        }
        opened
    }

    pub fn forget(&self, kind: &str, id: &str) {
        let _ = fs::remove_file(self.path(&self.name(kind, id)));
    }

    /// Drops what has aged out, then the oldest until the whole fits.
    pub fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
            .flatten()
            .filter_map(|entry| {
                let meta = entry.metadata().ok()?;
                Some((meta.modified().ok()?, meta.len(), entry.path()))
            })
            .collect();
        files.sort_by_key(|(modified, ..)| std::cmp::Reverse(*modified));
        let mut total = 0;
        for (modified, size, path) in files {
            total += size;
            let old = modified.elapsed().is_ok_and(|age| age > MOST_AGE);
            if old || total > MOST_BYTES {
                let _ = fs::remove_file(path);
            }
        }
    }
}

pub fn erase(dir: &Path) {
    let _ = fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("proton-store-{name}-{}", std::process::id()));
        erase(&dir);
        dir
    }

    #[test]
    fn what_goes_in_comes_out_and_is_not_readable_on_disk() {
        let dir = folder("round");
        let store = MailStore::open(&dir, new_key()).unwrap();
        store.put("mail", "id-1", &"a secret subject");
        assert_eq!(store.get::<String>("mail", "id-1").unwrap(), "a secret subject");
        let file = fs::read_dir(&dir).unwrap().next().unwrap().unwrap();
        let name = file.file_name().into_string().unwrap();
        assert!(!name.contains("id-1"));
        let bytes = fs::read(file.path()).unwrap();
        assert!(!bytes.windows(6).any(|part| part == b"secret"));
        erase(&dir);
    }

    #[test]
    fn another_key_opens_nothing_and_a_damaged_file_goes() {
        let dir = folder("keys");
        let key = new_key();
        let store = MailStore::open(&dir, key).unwrap();
        store.put("mail", "a", &1);
        let path = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(store.get::<u32>("mail", "a").is_none());
        assert!(!path.exists());
        store.put("mail", "a", &1);
        let stranger = MailStore::open(&dir, new_key()).unwrap();
        assert!(stranger.get::<u32>("mail", "a").is_none());
        erase(&dir);
    }

    #[test]
    fn a_file_cannot_pose_as_another() {
        let dir = folder("swap");
        let store = MailStore::open(&dir, new_key()).unwrap();
        store.put("mail", "a", &1);
        store.put("mail", "b", &2);
        fs::copy(store.path(&store.name("mail", "a")), store.path(&store.name("mail", "b"))).unwrap();
        assert!(store.get::<u32>("mail", "b").is_none());
        erase(&dir);
    }
}
