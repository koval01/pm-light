use super::*;

fn cipher() -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new_from_slice(new_key().as_slice()).unwrap()
}

fn record(text: &str) -> Record {
    Record { time: now_millis(), level: "info".into(), area: "test".into(), text: text.into() }
}

fn folder() -> PathBuf {
    let name = format!("pml-journal-{}-{}", std::process::id(), rand::random::<u64>());
    let dir = std::env::temp_dir().join(name);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn writer(dir: &Path, cipher: &XChaCha20Poly1305) -> Writer {
    Writer { dir: dir.to_path_buf(), cipher: cipher.clone(), current: None }
}

#[test]
fn nothing_readable_reaches_the_disk() {
    let (dir, cipher) = (folder(), cipher());
    writer(&dir, &cipher).write(&record("a very recognisable sentence"));
    let (_, path) = hour_files(&dir).pop().unwrap();
    let bytes = fs::read(path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("recognisable"));
    assert!(!String::from_utf8_lossy(&bytes).contains("info"));
}

#[test]
fn reads_back_newest_first() {
    let (dir, cipher) = (folder(), cipher());
    let mut writer = writer(&dir, &cipher);
    writer.write(&record("first"));
    writer.write(&record("second"));
    let texts: Vec<String> = read_from(&dir, &cipher, 10).into_iter().map(|r| r.text).collect();
    assert_eq!(texts, ["second", "first"]);
}

#[test]
fn stops_at_the_limit() {
    let (dir, cipher) = (folder(), cipher());
    let mut writer = writer(&dir, &cipher);
    for index in 0..5 {
        writer.write(&record(&index.to_string()));
    }
    let texts: Vec<String> = read_from(&dir, &cipher, 2).into_iter().map(|r| r.text).collect();
    assert_eq!(texts, ["4", "3"]);
}

#[test]
fn another_key_reads_nothing() {
    let (dir, cipher_a) = (folder(), cipher());
    writer(&dir, &cipher_a).write(&record("secret"));
    assert!(read_from(&dir, &cipher(), 10).is_empty());
}

#[test]
fn a_changed_record_is_dropped_and_the_rest_survive() {
    let (dir, cipher) = (folder(), cipher());
    let mut writer = writer(&dir, &cipher);
    writer.write(&record("first"));
    writer.write(&record("second"));
    let (_, path) = hour_files(&dir).pop().unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let last = LENGTH_BYTES + NONCE_BYTES + 2;
    bytes[last] ^= 1;
    fs::write(&path, bytes).unwrap();
    let texts: Vec<String> = read_from(&dir, &cipher, 10).into_iter().map(|r| r.text).collect();
    assert_eq!(texts, ["second"]);
}

#[test]
fn a_torn_end_costs_only_the_last_record() {
    let (dir, cipher) = (folder(), cipher());
    let mut writer = writer(&dir, &cipher);
    writer.write(&record("whole"));
    writer.write(&record("torn"));
    let (_, path) = hour_files(&dir).pop().unwrap();
    let bytes = fs::read(&path).unwrap();
    fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();
    let texts: Vec<String> = read_from(&dir, &cipher, 10).into_iter().map(|r| r.text).collect();
    assert_eq!(texts, ["whole"]);
}

#[test]
fn junk_cannot_panic_the_reader() {
    let (dir, cipher) = (folder(), cipher());
    fs::write(dir.join("1.journal"), [0xff; 64]).unwrap();
    fs::write(dir.join("2.journal"), [0x01, 0, 0, 0, 0]).unwrap();
    fs::write(dir.join("3.journal"), []).unwrap();
    assert!(read_from(&dir, &cipher, 10).is_empty());
}

#[test]
fn a_day_is_kept_and_what_is_older_goes() {
    let dir = folder();
    let now = 1000;
    for hour in [now - RETENTION_HOURS - 1, now - RETENTION_HOURS, now] {
        fs::write(dir.join(format!("{hour}.journal")), b"x").unwrap();
    }
    fs::write(dir.join("notes.txt"), b"not ours").unwrap();
    fs::write(dir.join("notes.journal"), b"not ours").unwrap();
    prune(&dir, now);
    let kept: Vec<u64> = hour_files(&dir).into_iter().map(|(hour, _)| hour).collect();
    assert_eq!(kept, [now - RETENTION_HOURS, now]);
    assert!(dir.join("notes.txt").exists() && dir.join("notes.journal").exists());
}

#[test]
fn records_from_a_session_a_week_ago_go_at_the_next_start() {
    let dir = folder();
    let now = hour_of(now_millis());
    let closed = now - 7 * 24;
    for hour in closed - 20..closed {
        fs::write(dir.join(format!("{hour}.journal")), b"x").unwrap();
    }
    prune(&dir, now);
    assert!(hour_files(&dir).is_empty());
}

#[test]
fn a_clock_set_back_does_not_keep_records_alive() {
    let dir = folder();
    fs::write(dir.join("1000.journal"), b"x").unwrap();
    fs::write(dir.join("1001.journal"), b"x").unwrap();
    fs::write(dir.join("1002.journal"), b"x").unwrap();
    prune(&dir, 1000);
    let kept: Vec<u64> = hour_files(&dir).into_iter().map(|(hour, _)| hour).collect();
    assert_eq!(kept, [1000, 1001]);
}

#[test]
fn a_new_hour_clears_what_has_expired() {
    let (dir, cipher) = (folder(), cipher());
    let old = hour_of(now_millis()) - RETENTION_HOURS - 2;
    fs::write(dir.join(format!("{old}.journal")), b"x").unwrap();
    writer(&dir, &cipher).write(&record("now"));
    assert!(!dir.join(format!("{old}.journal")).exists());
}

#[test]
fn a_full_hour_keeps_only_what_matters() {
    let (dir, cipher) = (folder(), cipher());
    let mut writer = writer(&dir, &cipher);
    writer.write(&record("start"));
    if let Some((_, _, size)) = writer.current.as_mut() {
        *size = HOUR_LIMIT;
    }
    writer.write(&record("chatter"));
    writer.write(&Record { level: "warn".into(), ..record("trouble") });
    let texts: Vec<String> = read_from(&dir, &cipher, 10).into_iter().map(|r| r.text).collect();
    assert_eq!(texts, ["trouble", "start"]);
}

#[test]
fn clearing_leaves_nothing() {
    let (dir, cipher) = (folder(), cipher());
    let mut writer = writer(&dir, &cipher);
    writer.write(&record("gone"));
    writer.clear();
    assert!(hour_files(&dir).is_empty());
    writer.write(&record("later"));
    assert_eq!(read_from(&dir, &cipher, 10).len(), 1);
}

#[cfg(unix)]
#[test]
fn only_the_owner_can_open_a_file() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, cipher) = (folder(), cipher());
    writer(&dir, &cipher).write(&record("private"));
    let (_, path) = hour_files(&dir).pop().unwrap();
    assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn paths_keep_their_shape_and_lose_their_ids() {
    assert_eq!(clean_path("/mail/v4/messages/read"), "/mail/v4/messages/read");
    assert_eq!(clean_path("/mail/v4/messages?Page=0&Address=a@b.c"), "/mail/v4/messages");
    assert_eq!(clean_path("/mail/v4/messages/aB3dE5gH7jK9lM1nO3=="), "/mail/v4/messages/{id}");
    assert_eq!(clean_path("/core/v4/keys/someone@example.com/all"), "/core/v4/keys/{id}/all");
    assert_eq!(clean_path("/mail/v4/attachments/a%2Fb"), "/mail/v4/attachments/{id}");
    assert_eq!(clean_path("/core/v4/organizations"), "/core/v4/organizations");
    assert_eq!(clean_path("/mail/v4/conversations/count"), "/mail/v4/conversations/count");
    assert_eq!(clean_path("/core/v4/domains/example.org"), "/core/v4/domains/{id}");
    assert_eq!(clean_path("/mail/v4/messages/abcdefghijklmnopqrstuvwxyzabc"), "/mail/v4/messages/{id}");
}

#[test]
fn long_text_is_cut_between_characters() {
    let clipped = clip(&"🦀".repeat(TEXT_LIMIT + 5));
    assert_eq!(clipped.chars().count(), TEXT_LIMIT);
}

#[test]
fn areas_are_short_and_third_parties_stay_out() {
    assert_eq!(area_of("proton_core::service::sign_in"), "service::sign_in");
    assert_eq!(area_of("proton_mail_light::sync"), "sync");
    assert_eq!(area_of("proton_page"), "page");
    assert_eq!(area_of("proton_mail_light"), "app");
    let logger = Logger { queue: sync_channel(1).0 };
    assert!(logger.enabled(&Metadata::builder().target("proton_core::client").build()));
    assert!(!logger.enabled(&Metadata::builder().target("hyper::proto").build()));
}

#[test]
fn tors_life_is_kept_without_its_noise() {
    let logger = Logger { queue: sync_channel(1).0 };
    let at = |target: &'static str, level: log::Level| Metadata::builder().target(target).level(level).build();
    assert!(logger.enabled(&at("tor_dirmgr::bootstrap", log::Level::Info)));
    assert!(logger.enabled(&at("tor_guardmgr::guard", log::Level::Warn)));
    assert!(logger.enabled(&at("arti_client::client", log::Level::Error)));
    assert!(!logger.enabled(&at("tor_proto::channel", log::Level::Debug)));
    assert!(!logger.enabled(&at("tor_circmgr", log::Level::Trace)));
    assert!(logger.enabled(&at("proton_core::tor", log::Level::Debug)));
    assert!(!logger.enabled(&at("rustls::client", log::Level::Error)));
    assert!(!logger.enabled(&at("torment::x", log::Level::Info)));
    assert_eq!(area_of("tor_dirmgr::bootstrap"), "tor_dirmgr");
    assert_eq!(area_of("arti_client::status"), "arti_client");
}
