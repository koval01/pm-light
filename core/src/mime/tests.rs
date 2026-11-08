use super::*;

#[test]
fn reads_a_plain_message() {
    let r = parse_mime(b"Content-Type: text/plain; charset=utf-8\r\n\r\nHello");
    assert_eq!(r.text.as_deref(), Some("Hello"));
}

#[test]
fn keeps_8_bit_text_intact() {
    let raw = "Content-Type: text/plain; charset=utf-8\n\nCafé 😀".as_bytes();
    assert_eq!(parse_mime(raw).text.as_deref(), Some("Café 😀"));
}

#[test]
fn decodes_a_legacy_charset() {
    let mut raw = b"Content-Type: text/plain; charset=windows-1252\n\n".to_vec();
    raw.extend([0x43, 0x61, 0x66, 0xe9]);
    assert_eq!(parse_mime(&raw).text.as_deref(), Some("Café"));
}

#[test]
fn an_unknown_charset_falls_back_to_utf8() {
    let raw = b"Content-Type: text/plain; charset=nonsense\n\nabc";
    assert_eq!(parse_mime(raw).text.as_deref(), Some("abc"));
}

#[test]
fn decodes_quoted_printable() {
    let raw = b"Content-Type: text/plain; charset=utf-8\nContent-Transfer-Encoding: quoted-printable\n\n=C3=A9=\nx=3Dy";
    assert_eq!(parse_mime(raw).text.as_deref(), Some("éx=y"));
}

#[test]
fn keeps_a_stray_equals_sign() {
    assert_eq!(decode_quoted_printable(b"a=zb="), b"a=zb=");
}

#[test]
fn decodes_base64_with_line_breaks() {
    let raw = b"Content-Type: text/plain\nContent-Transfer-Encoding: base64\n\naGVs\r\nbG8=";
    assert_eq!(parse_mime(raw).text.as_deref(), Some("hello"));
}

const MIXED: &[u8] = b"Content-Type: multipart/mixed; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: multipart/alternative; boundary=b2\r\n\r\n--b2\r\nContent-Type: text/plain\r\n\r\nplain\r\n--b2\r\nContent-Type: text/html\r\n\r\n<p>html</p>\r\n--b2--\r\n--b1\r\nContent-Type: application/pdf; name=\"a.pdf\"\r\nContent-Disposition: attachment; filename=\"a.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nAQID\r\n--b1\r\nContent-Type: image/png\r\nContent-ID: <img1>\r\nContent-Transfer-Encoding: base64\r\n\r\nBAU=\r\n--b1--\r\n";

#[test]
fn walks_nested_multiparts() {
    let r = parse_mime(MIXED);
    assert_eq!(r.text.as_deref(), Some("plain"));
    assert_eq!(r.html.as_deref(), Some("<p>html</p>"));
    assert_eq!(r.attachments.len(), 2);
}

#[test]
fn reads_attachment_bytes_and_names() {
    let r = parse_mime(MIXED);
    assert_eq!(r.attachments[0].name, "a.pdf");
    assert_eq!(r.attachments[0].data, [1, 2, 3]);
    assert!(!r.attachments[0].inline);
}

#[test]
fn a_content_id_makes_an_inline_image() {
    let r = parse_mime(MIXED);
    assert_eq!(r.attachments[1].content_id.as_deref(), Some("img1"));
    assert!(r.attachments[1].inline);
    assert_eq!(r.attachments[1].mime, "image/png");
}

#[test]
fn survives_a_missing_closing_boundary() {
    let raw = b"Content-Type: multipart/mixed; boundary=x\n\n--x\nContent-Type: text/plain\n\nbody";
    assert_eq!(parse_mime(raw).text.as_deref(), Some("body"));
}

#[test]
fn a_boundary_with_regex_characters_is_literal() {
    let raw = b"Content-Type: multipart/mixed; boundary=\"a+b(c\"\n\n--a+b(c\nContent-Type: text/plain\n\nx\n--a+b(c--";
    assert_eq!(parse_mime(raw).text.as_deref(), Some("x"));
}

#[test]
fn decodes_encoded_words() {
    assert_eq!(decode_encoded_words("=?UTF-8?B?Y2Fmw6k=?=.pdf"), "café.pdf");
    assert_eq!(decode_encoded_words("=?utf-8?q?a_b=3D?="), "a b=");
}

#[test]
fn reads_an_rfc_5987_file_name() {
    let d = "attachment; filename*=UTF-8''caf%C3%A9.txt";
    assert_eq!(file_name(d, "").as_deref(), Some("café.txt"));
}

#[test]
fn folded_headers_are_joined() {
    let raw = b"Content-Type: text/plain;\r\n charset=utf-8\r\n\r\nx";
    assert_eq!(parse_mime(raw).text.as_deref(), Some("x"));
}

#[test]
fn nesting_past_the_limit_stops_instead_of_overflowing_the_stack() {
    let mut raw = String::from("Content-Type: text/plain\n\nburied");
    for i in 0..5000 {
        raw = format!("Content-Type: multipart/mixed; boundary=b{i}\n\n--b{i}\n{raw}\n--b{i}--\n");
    }
    assert_eq!(parse_mime(raw.as_bytes()).text, None);
}

#[test]
fn a_flood_of_parts_is_cut_off() {
    let part = "--b\nContent-Type: application/x\nContent-Disposition: attachment; filename=a\n\nx\n";
    let raw = format!("Content-Type: multipart/mixed; boundary=b\n\n{}--b--", part.repeat(10_000));
    assert_eq!(parse_mime(raw.as_bytes()).attachments.len(), MAX_PARTS);
}

#[test]
fn junk_cannot_panic_the_parser() {
    for raw in [&b"\xff\xfe\x00"[..], b"Content-Type: multipart/mixed; boundary=", b":::\n\n", b""] {
        let _ = parse_mime(raw);
    }
    assert_eq!(decode_quoted_printable(b"=\xff=\xd0"), b"=\xff=\xd0");
}
