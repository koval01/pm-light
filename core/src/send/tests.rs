use super::*;

fn outgoing() -> Outgoing {
    Outgoing {
        draft_id: None,
        from: None,
        to: vec!["a@x.io".into()],
        cc: vec![],
        bcc: vec![],
        subject: "S".into(),
        body: "B".into(),
        mime: "text/html".into(),
        files: vec![],
        existing: vec![],
        parent_id: None,
        action: None,
        deliver_at: None,
        auto_save_contacts: None,
        delay_seconds: None,
        password: None,
        password_hint: None,
        expires_in: None,
        read_receipt: None,
    }
}

fn row(flags: u32, key: &str) -> PublicKeyRow {
    PublicKeyRow { flags, public_key: key.into() }
}

#[test]
fn a_draft_names_its_sender() {
    let payload = draft_payload(&outgoing(), "me@p.me", "Me", "ARMOR");
    assert_eq!(payload["Message"]["Sender"]["Address"], "me@p.me");
    assert_eq!(payload["Message"]["Body"], "ARMOR");
    assert_eq!(payload["Message"]["ToList"][0]["Address"], "a@x.io");
    assert!(payload.get("ParentID").is_none());
}

#[test]
fn a_reply_points_at_its_parent() {
    let mail = Outgoing { parent_id: Some("p".into()), action: Some(1), ..outgoing() };
    let payload = draft_payload(&mail, "me@p.me", "", "A");
    assert_eq!(payload["ParentID"], "p");
    assert_eq!(payload["Action"], 1);
}

#[test]
fn a_read_receipt_is_a_flag() {
    let mail = Outgoing { read_receipt: Some(true), ..outgoing() };
    assert_eq!(draft_payload(&mail, "m", "", "A")["Message"]["Flags"], 65536);
    assert!(draft_payload(&outgoing(), "m", "", "A")["Message"].get("Flags").is_none());
}

#[test]
fn encryption_skips_obsolete_and_barred_keys() {
    let keys = [row(1, "obsolete"), row(3 | 4, "barred"), row(3, "good")];
    assert_eq!(encryption_key_of(&keys), Some("good"));
    assert_eq!(encryption_key_of(&[row(1, "x")]), None);
}

#[test]
fn a_compromised_key_cannot_vouch_but_an_obsolete_one_can() {
    let keys = [row(1, "obsolete"), row(2, "compromised"), row(3, "good")];
    assert_eq!(trusted_keys_of(&keys), ["obsolete", "good"]);
}
