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

#[test]
fn the_same_mail_never_lies_on_disk_the_same_way() {
    let dir = folder("nonce");
    let store = MailStore::open(&dir, new_key()).unwrap();
    let read = || {
        let file = fs::read_dir(&dir).unwrap().next().unwrap().unwrap();
        fs::read(file.path()).unwrap()
    };
    store.put("mail", "id-1", &"the same");
    let first = read();
    store.put("mail", "id-1", &"the same");
    assert_ne!(first, read(), "each write has a nonce of its own");
    erase(&dir);
}

/// A store key as the Keychain keeps it, and a mail the store wrote with it before an update of what makes keys and
/// nonces (rand 0.8): the file's name, from SHA-256 of the key, the kind, a zero and the id, then its bytes, the nonce
/// and the sealed text.
const KEPT_KEY: &str = "5b0e3c9a71d24f86e1a03b57c9d82e146fa0b3c58d71e29f04a6b8c3d5e7f912";
const KEPT_NAME: &str = "8813093a74933bb60cc41e0e163aebd740e6f9d88b02261e4cb51fdea572779a";
const KEPT_MAIL: &str = "c8a7c21663a16bd39015697611ef7bbd986cf16fcb1eb2fe27819f083cf279c983d1d10fe9807fd19cd20988149d2beaf501314bbd35fde7228c5e2a2427a0af2f";

fn bytes_of(hex: &str) -> Vec<u8> {
    hex.as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect()
}

#[test]
fn a_mail_kept_before_an_update_still_opens() {
    let dir = folder("kept");
    let mut key = Zeroizing::new([0; KEY_BYTES]);
    key.copy_from_slice(&bytes_of(KEPT_KEY));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{KEPT_NAME}.{EXTENSION}")), bytes_of(KEPT_MAIL)).unwrap();
    let store = MailStore::open(&dir, key).unwrap();
    assert_eq!(store.get::<String>("mail", "id-1").as_deref(), Some("a mail kept from before"));
    erase(&dir);
}

#[test]
fn every_store_key_is_new_and_none_is_zero() {
    let keys: std::collections::BTreeSet<[u8; KEY_BYTES]> = (0..100).map(|_| *new_key()).collect();
    assert_eq!(keys.len(), 100);
    assert!(!keys.contains(&[0; KEY_BYTES]));
}
