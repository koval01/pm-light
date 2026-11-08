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
