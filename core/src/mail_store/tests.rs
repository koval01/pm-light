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
