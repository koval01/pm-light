//! Against Proton's own example key, so these prove more than self-consistency.

use super::*;
use crate::send::{self, PasswordProtection, Recipient, Uploaded};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use proton_crypto_account::{
    keys::{ArmoredPrivateKey, KeyFlag, KeyId, LocalAddressKey},
    proton_crypto::crypto::KeyGeneratorAlgorithm,
};

const LOCKED_USER_KEY: &str = include_str!("test_key.asc");
const USER_KEY_ID: &str = "aTdvCsWuv2V_YQQ5nLKsWPkHWMrlHfUxL9aTWakz6blhwI0q_j4MKnxO29xMQ4slCRvo3lFLE8ljb3kvMP2PQQ==";
const USER_KEY_SALT: &str = "6bIzN4A8bOwmsiEuCPj74g==";
const PASSWORD: &str = "password";
const ADDRESS: &str = "address-1";

fn locked_user_key() -> LockedKey {
    LockedKey {
        id: KeyId::from(USER_KEY_ID),
        version: 3,
        private_key: ArmoredPrivateKey::from(LOCKED_USER_KEY),
        token: None,
        signature: None,
        activation: None,
        primary: true,
        active: true,
        flags: None,
        recovery_secret: None,
        recovery_secret_signature: None,
        address_forwarding_id: None,
    }
}

fn salts() -> Vec<KeySaltRow> {
    vec![KeySaltRow { id: USER_KEY_ID.into(), key_salt: Some(USER_KEY_SALT.into()) }]
}

struct Fixture {
    crypto: Arc<dyn Crypto>,
    public_key: String,
}

/// A user key from Proton's example, with a fresh address key locked to it as the server would.
fn fixture() -> Fixture {
    let crypto = engine();
    let secret = crypto.key_secret(PASSWORD, &[locked_user_key()], &salts()).unwrap();
    let pgp = new_pgp_provider();
    let users = UserKeys::new(vec![locked_user_key()]).unlock(&pgp, &KeySecret::new(secret.clone())).unlocked_keys;
    let local = LocalAddressKey::generate(
        &pgp,
        "user@proton.me",
        KeyGeneratorAlgorithm::default(),
        KeyFlag::default(),
        true,
        &users[0],
    )
    .unwrap();
    let locked = LockedKey {
        id: KeyId::from("address-key"),
        version: 3,
        private_key: local.private_key,
        token: local.token,
        signature: local.signature,
        activation: None,
        primary: true,
        active: true,
        flags: Some(KeyFlag::default()),
        recovery_secret: None,
        recovery_secret_signature: None,
        address_forwarding_id: None,
    };
    let unlocked = AddressKeys::new(vec![locked.clone()]).unlock(&pgp, &users, None).unlocked_keys;
    let public_key = pgp.public_key_export(&unlocked[0].public_key, DataEncoding::Armor).unwrap();
    let public_key = String::from_utf8(public_key.as_ref().to_vec()).unwrap();
    crypto
        .unlock(vec![locked_user_key()], vec![AddressInput { id: ADDRESS.into(), keys: vec![locked] }], &secret)
        .unwrap();
    Fixture { crypto, public_key }
}

fn other_public_key() -> String {
    let pgp = new_pgp_provider();
    let secret = engine().key_secret(PASSWORD, &[locked_user_key()], &salts()).unwrap();
    let users = UserKeys::new(vec![locked_user_key()]).unlock(&pgp, &KeySecret::new(secret)).unlocked_keys;
    let exported = pgp.public_key_export(&users[0].public_key, DataEncoding::Armor).unwrap();
    String::from_utf8(exported.as_ref().to_vec()).unwrap()
}

#[test]
fn opens_proton_example_key_with_its_salt_and_password() {
    assert!(fixture().crypto.has_keys_for(ADDRESS));
}

#[test]
fn a_wrong_password_opens_nothing() {
    let crypto = engine();
    let secret = crypto.key_secret("wrong", &[locked_user_key()], &salts()).unwrap();
    assert!(crypto.unlock(vec![locked_user_key()], vec![], &secret).is_err());
}

#[test]
fn an_account_without_a_salt_uses_the_password_itself() {
    let secret = engine().key_secret("plain", &[locked_user_key()], &[]).unwrap();
    assert_eq!(secret, b"plain");
}

#[test]
fn inactive_keys_stay_locked() {
    let crypto = engine();
    let secret = crypto.key_secret(PASSWORD, &[locked_user_key()], &salts()).unwrap();
    let inactive = LockedKey { active: false, ..locked_user_key() };
    assert!(crypto.unlock(vec![inactive], vec![], &secret).is_err());
}

#[test]
fn a_body_survives_the_round_trip_with_a_valid_signature() {
    let f = fixture();
    let text = "<p>Café 😀</p>";
    let material = f.crypto.encrypt_body(text, ADDRESS, None).unwrap();
    let opened = f.crypto.decrypt_message(&material.armored, ADDRESS, std::slice::from_ref(&f.public_key)).unwrap();
    assert_eq!(opened.data, text.as_bytes());
    assert_eq!(opened.signature, SignatureStatus::Valid);
}

#[test]
fn a_stranger_key_makes_a_signature_invalid_not_valid() {
    let f = fixture();
    let material = f.crypto.encrypt_body("x", ADDRESS, None).unwrap();
    let opened = f.crypto.decrypt_message(&material.armored, ADDRESS, &[other_public_key()]).unwrap();
    assert_ne!(opened.signature, SignatureStatus::Valid);
}

#[test]
fn with_no_key_to_check_a_signature_stays_unknown() {
    let f = fixture();
    let material = f.crypto.encrypt_body("x", ADDRESS, None).unwrap();
    let opened = f.crypto.decrypt_message(&material.armored, ADDRESS, &[]).unwrap();
    assert_eq!(opened.signature, SignatureStatus::Unknown);
}

#[test]
fn a_scheduled_body_is_signed_for_its_delivery_time() {
    let f = fixture();
    let later = seconds_now() + 3600;
    let material = f.crypto.encrypt_body("x", ADDRESS, Some(later)).unwrap();
    let opened = f.crypto.decrypt_message(&material.armored, ADDRESS, std::slice::from_ref(&f.public_key)).unwrap();
    assert_eq!(opened.signature, SignatureStatus::Valid);
}

#[test]
fn an_attachment_survives_the_round_trip() {
    let f = fixture();
    let data: Vec<u8> = (0..=255).cycle().take(100_000).collect();
    let a = f.crypto.encrypt_attachment(&data, ADDRESS).unwrap();
    assert_eq!(f.crypto.decrypt_attachment(&a.key_packets, &a.data_packet, ADDRESS).unwrap(), data);
    assert!(!a.signature.is_empty());
}

#[test]
fn a_session_key_can_be_recovered_from_its_key_packet() {
    let f = fixture();
    let a = f.crypto.encrypt_attachment(b"x", ADDRESS).unwrap();
    let recovered = f.crypto.session_key(&a.key_packets, ADDRESS).unwrap();
    assert_eq!(recovered.key, a.session_key.key);
    assert_eq!(recovered.algorithm, AES256);
}

#[test]
fn a_rewrapped_session_key_opens_the_shared_data_packet() {
    let f = fixture();
    let material = f.crypto.encrypt_body("shared", ADDRESS, None).unwrap();
    let packet = f.crypto.wrap_for_key(&material.session_key, &f.public_key).unwrap();
    let opened = f.crypto.decrypt_attachment(&packet, &material.data_packet, ADDRESS).unwrap();
    assert_eq!(opened, b"shared");
}

#[test]
fn a_password_opens_a_session_key_and_a_wrong_one_does_not() {
    let f = fixture();
    let material = f.crypto.encrypt_body("secret", ADDRESS, None).unwrap();
    let packet = f.crypto.wrap_for_password(&material.session_key, "correct horse").unwrap();
    let pgp = new_pgp_provider();
    let opened = pgp.new_decryptor().with_passphrase("correct horse").decrypt_session_key(&packet).unwrap();
    let (key, _) = pgp.session_key_export(&opened).unwrap();
    assert_eq!(key.as_ref(), &material.session_key.key[..]);
    // A packet sealed by a password carries no check of its own (OpenPGP's SKESK v4): a wrong password is usually
    // refused, but now and then opens it into some other key. It never yields the real one, and the mail it holds
    // stays shut, since its data packet is checked.
    for _ in 0..3 {
        let material = f.crypto.encrypt_body("secret", ADDRESS, None).unwrap();
        let packet = f.crypto.wrap_for_password(&material.session_key, "correct horse").unwrap();
        if let Ok(opened) = pgp.new_decryptor().with_passphrase("wrong").decrypt_session_key(&packet) {
            let (key, _) = pgp.session_key_export(&opened).unwrap();
            assert_ne!(key.as_ref(), &material.session_key.key[..], "a wrong password opened the real key");
        }
    }
}

#[test]
fn enc_token_hides_the_token_under_the_password() {
    let f = fixture();
    let armored = f.crypto.encrypt_for_password("token", "pw").unwrap();
    let opened =
        new_pgp_provider().new_decryptor().with_passphrase("pw").decrypt(&armored, DataEncoding::Armor).unwrap();
    assert_eq!(opened.into_vec(), b"token");
}

#[test]
fn a_detached_contact_signature_is_armored() {
    let signature = fixture().crypto.sign_with_user_key("BEGIN:VCARD\r\n").unwrap();
    assert!(signature.starts_with("-----BEGIN PGP SIGNATURE-----"));
}

#[test]
fn signing_needs_the_address_keys() {
    let f = fixture();
    f.crypto.forget_keys();
    assert!(f.crypto.encrypt_body("x", ADDRESS, None).is_err());
    assert!(!f.crypto.has_keys_for(ADDRESS));
}

#[test]
fn proton_signed_modulus_gives_a_verifier() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!("test_vectors.json")).unwrap();
    let modulus = vectors["modulus"].as_str().unwrap();
    let (salt, verifier) = engine().password_verifier("pw", modulus).unwrap();
    assert_eq!(STANDARD.decode(salt).unwrap().len(), 10);
    assert_eq!(STANDARD.decode(verifier).unwrap().len(), 256);
}

#[test]
fn a_forged_modulus_is_refused() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!("test_vectors.json")).unwrap();
    let forged = vectors["modulus"].as_str().unwrap().replacen("y6Tt", "z6Tt", 1);
    assert!(engine().password_verifier("pw", &forged).is_err());
}

#[test]
fn login_proofs_are_well_formed() {
    let vectors: serde_json::Value = serde_json::from_str(include_str!("test_vectors.json")).unwrap();
    let info = AuthInfo {
        version: 4,
        modulus: vectors["modulus"].as_str().unwrap().into(),
        server_ephemeral: vectors["serverEphemeral"].as_str().unwrap().into(),
        salt: vectors["salt"].as_str().unwrap().into(),
    };
    let proof = engine().login_proof("user", "123", &info).unwrap();
    assert_eq!(STANDARD.decode(proof.proof).unwrap().len(), 256);
    assert_eq!(STANDARD.decode(proof.ephemeral).unwrap().len(), 256);
}

fn package_for(f: &Fixture, recipients: &[Recipient], protection: Option<&PasswordProtection>) -> serde_json::Value {
    let body = f.crypto.encrypt_body("<p>x</p>", ADDRESS, None).unwrap();
    let attachment = f.crypto.encrypt_attachment(b"file", ADDRESS).unwrap();
    let uploaded = [Uploaded { id: "A".into(), session_key: attachment.session_key, signed: true }];
    send::build_package(f.crypto.as_ref(), "text/html", &body, recipients, &uploaded, protection).unwrap()
}

#[test]
fn an_internal_recipient_gets_wrapped_keys_and_no_key_in_the_clear() {
    let f = fixture();
    let recipients = [Recipient { email: "b@p.me".into(), public_key: Some(f.public_key.clone()) }];
    let package = package_for(&f, &recipients, None);
    assert_eq!(package["Type"], send::SCHEME_INTERNAL);
    assert_eq!(package["Addresses"]["b@p.me"]["Signature"], 1);
    assert!(package["Addresses"]["b@p.me"]["AttachmentKeyPackets"]["A"].is_string());
    assert!(package.get("BodyKey").is_none());
}

#[test]
fn an_outside_recipient_makes_the_package_carry_the_key() {
    let f = fixture();
    let recipients = [Recipient { email: "x@gmail.com".into(), public_key: None }];
    let package = package_for(&f, &recipients, None);
    assert_eq!(package["Type"], send::SCHEME_CLEAR);
    assert_eq!(package["BodyKey"]["Algorithm"], AES256);
    assert!(package["AttachmentKeys"]["A"]["Key"].is_string());
}

#[test]
fn a_mixed_send_combines_the_schemes() {
    let f = fixture();
    let recipients = [
        Recipient { email: "b@p.me".into(), public_key: Some(f.public_key.clone()) },
        Recipient { email: "x@gmail.com".into(), public_key: None },
    ];
    assert_eq!(package_for(&f, &recipients, None)["Type"], send::SCHEME_INTERNAL | send::SCHEME_CLEAR);
}

#[test]
fn a_password_recipient_gets_everything_proton_expects() {
    let f = fixture();
    let vectors: serde_json::Value = serde_json::from_str(include_str!("test_vectors.json")).unwrap();
    let protection = PasswordProtection {
        password: "pw".into(),
        hint: "usual".into(),
        modulus_id: "m".into(),
        signed_modulus: vectors["modulus"].as_str().unwrap().into(),
    };
    let recipients = [
        Recipient { email: "x@gmail.com".into(), public_key: None },
        Recipient { email: "y@gmail.com".into(), public_key: None },
    ];
    let package = package_for(&f, &recipients, Some(&protection));
    let entry = &package["Addresses"]["x@gmail.com"];
    assert_eq!(package["Type"], send::SCHEME_ENCRYPTED_OUTSIDE);
    assert!(package.get("BodyKey").is_none());
    assert_eq!(entry["Auth"]["Version"], 4);
    assert_eq!(entry["Auth"]["ModulusID"], "m");
    assert_eq!(entry["PasswordHint"], "usual");
    assert!(entry["EncToken"].as_str().unwrap().starts_with("-----BEGIN PGP MESSAGE-----"));
    assert_ne!(entry["Token"], package["Addresses"]["y@gmail.com"]["Token"]);
}

#[test]
fn nobody_to_send_to_is_an_error() {
    let f = fixture();
    let body = f.crypto.encrypt_body("x", ADDRESS, None).unwrap();
    assert!(send::build_package(f.crypto.as_ref(), "text/html", &body, &[], &[], None).is_err());
}

#[test]
fn a_clear_signature_is_the_standard_format_and_reads_back() {
    let f = fixture();
    let signed = f.crypto.clearsign("From: a@b.c\n\nHello", ADDRESS).unwrap();
    assert!(signed.starts_with("-----BEGIN PGP SIGNED MESSAGE-----"));
    assert!(signed.contains("-----BEGIN PGP SIGNATURE-----"));
    let read = f.crypto.clearsigned_text(&signed, std::slice::from_ref(&f.public_key)).unwrap();
    assert_eq!(read.trim_end(), "From: a@b.c\n\nHello");
}

#[test]
fn a_clear_signature_holds_only_for_its_text_and_key() {
    let f = fixture();
    let signed = f.crypto.clearsign("Pay 100", ADDRESS).unwrap();
    let changed = signed.replace("Pay 100", "Pay 900");
    assert_eq!(f.crypto.clearsigned_text(&changed, std::slice::from_ref(&f.public_key)), None);
    assert_eq!(f.crypto.clearsigned_text(&signed, &[other_public_key()]), None);
    assert_eq!(f.crypto.clearsigned_text(&signed, &[]), None);
    assert_eq!(f.crypto.clearsigned_text("junk", std::slice::from_ref(&f.public_key)), None);
}
