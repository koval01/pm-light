//! A message as an `.eml` file any mail program opens.
//!
//! Proton keeps the headers as they arrived and the parts apart, so the file is put back together: the headers that
//! described the old structure give way to ones that describe this one. Every part is base64, so no line can pass
//! for a boundary.

use crate::model::{Attachment, Message};
use std::fmt::Write;
use base64::{Engine, engine::general_purpose::STANDARD};

const BOUNDARY: &str = "=_proton-mail-light";
const LINE: usize = 76;
const FALLBACK_MIME: &str = "application/octet-stream";
const REPLACED: [&str; 3] = ["content-type", "content-transfer-encoding", "mime-version"];

/// The received headers, without those that spoke of the parts as they first arrived.
fn kept_headers(raw: &str) -> String {
    let mut out = String::new();
    let mut keeping = true;
    for line in raw.lines() {
        if !line.starts_with([' ', '\t']) {
            let name = line.split_once(':').map_or("", |(name, _)| name).trim().to_ascii_lowercase();
            keeping = !line.trim().is_empty() && !REPLACED.contains(&name.as_str());
        }
        if keeping {
            out.push_str(line);
            out.push_str("\r\n");
        }
    }
    out
}

fn base64_lines(data: &[u8]) -> String {
    let encoded = STANDARD.encode(data);
    let mut out = String::with_capacity(encoded.len() + encoded.len() / LINE * 2 + 2);
    for line in encoded.as_bytes().chunks(LINE) {
        out.push_str(&String::from_utf8_lossy(line));
        out.push_str("\r\n");
    }
    out
}

/// A type comes from whoever sent the mail, and it goes into a header line.
fn safe_mime(mime: &str) -> &str {
    let allowed = |byte: u8| byte.is_ascii_alphanumeric() || b"/+.-_".contains(&byte);
    if !mime.is_empty() && mime.bytes().all(allowed) { mime } else { FALLBACK_MIME }
}

/// RFC 2231: whatever the name holds, nothing of it can end the header line.
fn encoded_name(name: &str) -> String {
    let mut out = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn body_part(message: &Message) -> String {
    let kind = if message.html { "text/html" } else { "text/plain" };
    let body = base64_lines(message.body.as_bytes());
    format!("Content-Type: {kind}; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{body}")
}

fn file_part(file: &Attachment, data: &[u8]) -> String {
    let disposition = if file.inline { "inline" } else { "attachment" };
    let mut part = format!(
        "Content-Type: {}\r\nContent-Transfer-Encoding: base64\r\nContent-Disposition: {disposition}; filename*=UTF-8''{}\r\n",
        safe_mime(&file.mime),
        encoded_name(&file.name),
    );
    let id = file.content_id.as_deref().map(|id| id.trim_matches(['<', '>']));
    if let Some(id) = id.filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'<' && byte != b'>')) {
        let _ = write!(part, "Content-ID: <{id}>\r\n");
    }
    part.push_str("\r\n");
    part.push_str(&base64_lines(data));
    part
}

#[must_use]
pub fn build(message: &Message, files: &[(Attachment, Vec<u8>)]) -> Vec<u8> {
    let mut out = kept_headers(&message.raw_headers);
    out.push_str("MIME-Version: 1.0\r\n");
    if files.is_empty() {
        out.push_str(&body_part(message));
        return out.into_bytes();
    }
    // Writing to a String cannot fail.
    let _ = write!(out, "Content-Type: multipart/mixed; boundary=\"{BOUNDARY}\"\r\n\r\n");
    let _ = write!(out, "--{BOUNDARY}\r\n{}", body_part(message));
    for (file, data) in files {
        let _ = write!(out, "--{BOUNDARY}\r\n{}", file_part(file, data));
    }
    let _ = write!(out, "--{BOUNDARY}--\r\n");
    out.into_bytes()
}

#[cfg(test)]
mod tests;
