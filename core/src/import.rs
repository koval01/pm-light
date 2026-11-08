//! Mail put into the account as if it had arrived, as Proton's Import-Export app and Bridge put it there.
//!
//! As `go-proton-api` does it in `ImportMessages` and `EncryptRFC822`: the whole message, encrypted to and signed by
//! the address's key, as PGP/MIME, with the headers a list needs copied outside. Nothing is sent to anyone.

use std::fmt::Write as _;

/// The headers `encryptFull` keeps outside the encrypted part, for the list and the search to read.
const KEPT: [&str; 7] = ["Message-Id", "From", "To", "Subject", "Date", "Received", "Cc"];

/// The header of a message, its lines unfolded, as `(name, value)`.
fn header_lines(literal: &str) -> Vec<(String, String)> {
    let end = literal.find("\r\n\r\n").or_else(|| literal.find("\n\n")).unwrap_or(literal.len());
    let header = literal.get(..end).unwrap_or_default();
    let mut lines: Vec<(String, String)> = Vec::new();
    for line in header.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = lines.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            lines.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    lines
}

/// The value of `name`, whatever its case, and with nothing that could end the line it is written on.
fn header(lines: &[(String, String)], name: &str) -> Option<String> {
    lines
        .iter()
        .find(|(found, _)| found.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.replace(['\r', '\n'], " "))
}

/// The message as Proton imports it: `multipart/encrypted`, the `Version: 1` part, then `armored`, the whole of
/// `literal` encrypted to the address. `boundary` separates the parts, and must not occur in `armored`.
#[must_use]
pub fn pgp_mime(literal: &str, armored: &str, boundary: &str) -> String {
    let lines = header_lines(literal);
    let mut out = String::new();
    let mut put = |name: &str, value: &str| {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    };
    if let Some(id) = header(&lines, "Message-Id") {
        put("Message-Id", &id);
    }
    put("Mime-Version", "1.0");
    put("Content-Type", &format!("multipart/encrypted; boundary=\"{boundary}\"; protocol=\"application/pgp-encrypted\""));
    for name in KEPT.iter().skip(1) {
        if let Some(value) = header(&lines, name) {
            put(name, &value);
        }
    }
    out.push_str("\r\n");
    let _ = write!(
        out,
        "--{boundary}\r\nContent-Description: PGP/MIME version identification\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\
         --{boundary}\r\nContent-Description: OpenPGP encrypted message\r\nContent-Disposition: inline; filename=encrypted.asc\r\n\
         Content-Type: application/octet-stream; name=encrypted.asc\r\n\r\n{armored}\r\n--{boundary}--\r\n"
    );
    out
}

#[cfg(test)]
mod tests;
