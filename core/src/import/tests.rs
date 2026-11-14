use super::*;

const LITERAL: &str = "From: Anna <anna@example.org>\r\nTo: me@proton.me\r\nSubject: A long\r\n subject\r\nDate: Mon, 9 Nov 2026 10:00:00 +0000\r\nMessage-Id: <1@example.org>\r\nX-Other: left out\r\nContent-Type: text/plain\r\n\r\nHello\r\n";

#[test]
fn the_headers_a_list_needs_stay_outside() {
    let wrapped = pgp_mime(LITERAL, "-----BEGIN PGP MESSAGE-----\n...\n-----END PGP MESSAGE-----", "b0");
    assert!(wrapped.starts_with("Message-Id: <1@example.org>\r\nMime-Version: 1.0\r\n"));
    assert!(wrapped.contains("From: Anna <anna@example.org>\r\n"));
    assert!(wrapped.contains("Subject: A long subject\r\n"), "folded lines are joined");
    assert!(wrapped.contains("Date: Mon, 9 Nov 2026 10:00:00 +0000\r\n"));
    assert!(!wrapped.contains("X-Other"), "only the kept headers go outside");
    assert!(!wrapped.contains("Hello"), "the body travels only encrypted");
}

#[test]
fn the_parts_are_pgp_mime() {
    let wrapped = pgp_mime(LITERAL, "ARMOR", "b0");
    assert!(
        wrapped.contains("Content-Type: multipart/encrypted; boundary=\"b0\"; protocol=\"application/pgp-encrypted\"")
    );
    assert!(wrapped.contains("--b0\r\nContent-Description: PGP/MIME version identification\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n"));
    assert!(wrapped.contains("filename=encrypted.asc\r\nContent-Type: application/octet-stream; name=encrypted.asc\r\n\r\nARMOR\r\n--b0--\r\n"));
}

#[test]
fn a_header_cannot_add_lines_of_its_own() {
    let hostile = "Subject: x\r\nFrom: a@b.c\nBcc: injected@evil.example\r\n\r\nbody";
    let wrapped = pgp_mime(hostile, "A", "b0");
    assert!(!wrapped.contains("Bcc"));
    let header_end = wrapped.find("\r\n\r\n").unwrap_or(0);
    assert!(wrapped.get(..header_end).unwrap_or_default().lines().all(|line| line.contains(": ")));
}

#[test]
fn a_message_without_a_header_still_wraps() {
    let wrapped = pgp_mime("just text", "A", "b0");
    assert!(wrapped.starts_with("Mime-Version: 1.0\r\n"));
}
