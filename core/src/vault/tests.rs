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

#[test]
fn every_salt_is_new_and_none_is_zero() {
    assert_ne!(new_salt(), new_salt());
    assert_ne!(new_salt(), [0; SALT_BYTES]);
}

/// What a person could have set as the app password, and the salt beside it.
const PASSWORD: &str = "café password";
const SALT: [u8; SALT_BYTES] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// The keys for PASSWORD and SALT, from OpenSSL's Argon2id, which gives RFC 9106's own vector:
/// `openssl kdf -keylen 32 -kdfopt hexpass:636166c3a92070617373776f7264 -kdfopt hexsalt:000102030405060708090a0b0c0d0e0f
/// -kdfopt iter:3 -kdfopt memcost:65536 -kdfopt lanes:1 ARGON2ID`, and with iter:1 and memcost:256 for the tests' cost.
const APP_COST_KEY: &str = "70b994e12add2c64d5a49343649e0de1c2e2908b298a290a08949e67ec7f186c";
const TEST_COST_KEY: &str = "3cea240d6c35c85db570e3ba11c498754af890b78d0823ee0dda542051227094";

#[test]
fn the_apps_key_is_standard_argon2id() {
    let key = derive_key_at(PASSWORD, &SALT, APP_MEMORY_KIB, APP_PASSES).unwrap();
    assert_eq!(crate::hex(key.as_slice()), APP_COST_KEY);
}

#[test]
fn the_tests_key_is_standard_argon2id_too() {
    assert_eq!(crate::hex(derive_key(PASSWORD, &SALT).unwrap().as_slice()), TEST_COST_KEY);
}

/// The session as the Keychain keeps it, sealed by the app at its own cost, with argon2 0.5.3, before the move to 0.6.
const SEALED_BEFORE: &str = r#"{"salt":"AAECAwQFBgcICQoLDA0ODw==","nonce":"b5uBi7QRg8/W6joZnU9exAMFz8yISv2f","sealed":"nTsNzVhwj+T1KV00r7J0vJJEL4r+zyQ="}"#;

#[test]
fn a_session_sealed_before_an_update_still_opens() {
    let envelope: Envelope = serde_json::from_str(SEALED_BEFORE).unwrap();
    let key = derive_key_at(PASSWORD, &salt_of(&envelope).unwrap(), APP_MEMORY_KIB, APP_PASSES).unwrap();
    assert_eq!(open(&key, &envelope).unwrap().as_slice(), b"session");
}
