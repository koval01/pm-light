use super::*;

const RAW: &str = "Return-Path: <service@paypal.de>\r\n\
        Authentication-Results: mail.protonmail.ch; dmarc=pass (p=reject dis=none)\r\n header.from=paypal.de\r\n\
        Authentication-Results: mail.protonmail.ch; spf=pass smtp.mailfrom=paypal.de\r\n\
        Authentication-Results: mail.protonmail.ch; dkim=fail (2048-bit key)\r\n\
        Authentication-Results: mail.protonmail.ch; dkim=pass header.d=x\r\n\
        Message-Id: <4F.18@ccg13mail02>\r\n\
        X-Pm-Content-Encryption: on-delivery";

#[test]
fn joins_folded_lines() {
    assert!(header_values(RAW, "Authentication-Results")[0].contains("dis=none) header.from"));
}

#[test]
fn does_not_match_a_longer_header_name() {
    assert!(header_values("X-Foo-Bar: 1", "X-Foo").is_empty());
}

#[test]
fn a_header_name_in_tricky_unicode_is_skipped_not_a_crash() {
    // The Kelvin sign lowercases to a plain k, three bytes shorter or longer than it looks.
    assert!(header_values("\u{212a}: 1", "k").is_empty());
    assert_eq!(header_values("\u{212a}ey: 1\nkey: 2", "Key"), ["2"]);
}

#[test]
fn reads_every_mechanism_once_keeping_the_first_verdict() {
    let results = details_of(RAW).auth;
    let pairs: Vec<_> = results.iter().map(|r| (r.method.as_str(), r.result.as_str())).collect();
    assert_eq!(pairs, [("dkim", "fail"), ("dmarc", "pass"), ("spf", "pass")]);
}

#[test]
fn reads_the_encryption_and_message_id() {
    let details = details_of(RAW);
    assert_eq!(details.encryption, "onDelivery");
    assert_eq!(details.message_id, "<4F.18@ccg13mail02>");
}

#[test]
fn only_says_stored_when_unsure() {
    assert_eq!(encryption_of(""), "stored");
    assert_eq!(encryption_of("X-Pm-Content-Encryption: something-new"), "stored");
    assert_eq!(encryption_of("X-Pm-Content-Encryption: end-to-end"), "endToEnd");
}
