//! Reads a decrypted PGP/MIME body: the first text and HTML parts, and every file.

use crate::pattern::{BytesPattern, Pattern, group};
use base64::{Engine, engine::general_purpose::STANDARD};
use encoding_rs::Encoding;
use regex::Regex;
use regex::bytes::Regex as BytesRegex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MimeAttachment {
    pub name: String,
    pub mime: String,
    pub content_id: Option<String>,
    pub inline: bool,
    #[serde(with = "as_text")]
    pub data: Vec<u8>,
}

/// Files go to the disk as JSON, where a list of numbers would take four times the room.
mod as_text {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(data: &[u8], out: S) -> Result<S::Ok, S::Error> {
        out.serialize_str(&STANDARD.encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Vec<u8>, D::Error> {
        STANDARD.decode(String::deserialize(input)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MimeResult {
    pub text: Option<String>,
    pub html: Option<String>,
    pub attachments: Vec<MimeAttachment>,
}

const MAX_DEPTH: usize = 32;
const MAX_PARTS: usize = 2000;

pub(crate) static ENCODED_WORD: Pattern = Pattern::new(r"(?i)=\?([^?]+)\?([bq])\?([^?]*)\?=");
pub(crate) static FOLDED: BytesPattern = BytesPattern::new(r"\r?\n[ \t]+");
pub(crate) static BLANK_LINE: BytesPattern = BytesPattern::new(r"\r?\n\r?\n");
pub(crate) static EXTENDED: Pattern = Pattern::new(r"(?i)filename\*\s*=\s*([^']*)'[^']*'([^;\s]+)");

fn parse_headers(block: &[u8]) -> HashMap<String, String> {
    let unfolded = FOLDED.replace_all(block, &b" "[..]);
    String::from_utf8_lossy(&unfolded)
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            (!name.is_empty()).then(|| (name.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

fn split_header_body(raw: &[u8]) -> (&[u8], &[u8]) {
    BLANK_LINE.find(raw).map_or((raw, &[]), |blank| {
        (raw.get(..blank.start()).unwrap_or_default(), raw.get(blank.end()..).unwrap_or_default())
    })
}

fn header_param(value: &str, name: &str) -> Option<String> {
    let pattern = format!(r#"(?i)(?:^|;)\s*{}\s*=\s*(?:"([^"]*)"|([^;\s]+))"#, regex::escape(name));
    let captures = Regex::new(&pattern).ok()?.captures(value)?;
    captures.get(1).or_else(|| captures.get(2)).map(|m| m.as_str().to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

fn decode_quoted_printable(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while let Some(&byte) = text.get(i) {
        if byte != b'=' {
            out.push(byte);
            i += 1;
            continue;
        }
        match (text.get(i + 1), text.get(i + 2)) {
            (Some(b'\r'), Some(b'\n')) => i += 3,
            (Some(b'\n'), _) => i += 2,
            (Some(&high), Some(&low)) => {
                if let (Some(high), Some(low)) = (hex_value(high), hex_value(low)) {
                    out.push(high << 4 | low);
                    i += 3;
                } else {
                    out.push(b'=');
                    i += 1;
                }
            }
            _ => {
                out.push(b'=');
                i += 1;
            }
        }
    }
    out
}

fn decode_base64(text: &[u8]) -> Vec<u8> {
    let clean: Vec<u8> = text.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    STANDARD.decode(clean).unwrap_or_default()
}

fn decode_transfer(body: &[u8], encoding: Option<&str>) -> Vec<u8> {
    match encoding.unwrap_or_default().to_lowercase().as_str() {
        "base64" => decode_base64(body),
        "quoted-printable" => decode_quoted_printable(body),
        _ => body.to_vec(), // 7bit, 8bit, binary
    }
}

fn decode_charset(bytes: &[u8], charset: Option<&str>) -> String {
    let encoding = charset.and_then(|c| Encoding::for_label(c.as_bytes())).unwrap_or(encoding_rs::UTF_8);
    encoding.decode(bytes).0.into_owned()
}

/// RFC 2047: =?charset?B?...?= and =?charset?Q?...?= in file names.
pub fn decode_encoded_words(value: &str) -> String {
    ENCODED_WORD
        .replace_all(value, |captures: &regex::Captures<'_>| {
            let text = group(captures, 3).as_bytes();
            let bytes = if group(captures, 2).eq_ignore_ascii_case("b") {
                decode_base64(text)
            } else {
                let spaced: Vec<u8> = text.iter().map(|b| if *b == b'_' { b' ' } else { *b }).collect();
                decode_quoted_printable(&spaced)
            };
            decode_charset(&bytes, Some(group(captures, 1)))
        })
        .into_owned()
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let hex = (byte == b'%')
            .then(|| Some(hex_value(*bytes.get(i + 1)?)? << 4 | hex_value(*bytes.get(i + 2)?)?))
            .flatten();
        if let Some(byte) = hex {
            out.push(byte);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    out
}

fn file_name(disposition: &str, content_type: &str) -> Option<String> {
    if let Some(captures) = EXTENDED.captures(disposition) {
        return Some(decode_charset(&percent_decode(group(&captures, 2)), Some(group(&captures, 1))));
    }
    let plain = header_param(disposition, "filename").or_else(|| header_param(content_type, "name"))?;
    Some(decode_encoded_words(&plain))
}

fn split_parts<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let pattern = format!(r"(?:\A|\r?\n)--{}(--)?[ \t]*(?:\r?\n|\z)", regex::escape(boundary));
    let Ok(delimiter) = BytesRegex::new(&pattern) else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    let mut start = None;
    for captures in delimiter.captures_iter(body) {
        let Some(whole) = captures.get(0) else { continue };
        if let Some(from) = start {
            parts.extend(body.get(from..whole.start()));
        }
        if captures.get(1).is_some() {
            return parts;
        }
        start = Some(whole.end());
    }
    if let Some(from) = start {
        parts.extend(body.get(from..)); // unterminated multipart
    }
    parts
}

fn collect_leaves(raw: &[u8], out: &mut MimeResult, depth: usize) {
    let (head, body) = split_header_body(raw);
    let headers = parse_headers(head);
    let content_type = headers.get("content-type").map_or("text/plain", String::as_str);
    let kind = content_type.split(';').next().unwrap_or_default().trim().to_lowercase();

    if kind.starts_with("multipart/") {
        if let Some(boundary) = header_param(content_type, "boundary") {
            // Nesting has no legitimate use this deep, and recursion must not run out of stack.
            if depth < MAX_DEPTH {
                for part in split_parts(body, &boundary).into_iter().take(MAX_PARTS) {
                    collect_leaves(part, out, depth + 1);
                }
            }
        }
        return;
    }

    let disposition = headers.get("content-disposition").map_or("", String::as_str);
    let name = file_name(disposition, content_type);
    let content_id = headers.get("content-id").map(|id| id.trim_start_matches('<').trim_end_matches('>').to_string());
    let data = decode_transfer(body, headers.get("content-transfer-encoding").map(String::as_str));
    let lower = disposition.to_lowercase();
    let is_file = lower.starts_with("attachment") || name.is_some() || content_id.is_some();
    let charset = header_param(content_type, "charset");

    if !is_file && kind == "text/html" {
        out.html.get_or_insert_with(|| decode_charset(&data, charset.as_deref()));
    } else if !is_file && kind == "text/plain" {
        out.text.get_or_insert_with(|| decode_charset(&data, charset.as_deref()));
    } else if is_file || !kind.starts_with("text/") {
        out.attachments.push(MimeAttachment {
            name: name.unwrap_or_else(|| "attachment".to_string()),
            mime: kind,
            inline: lower.starts_with("inline") || (content_id.is_some() && disposition.is_empty()),
            content_id,
            data,
        });
    }
}

#[must_use]
pub fn parse_mime(raw: &[u8]) -> MimeResult {
    let mut out = MimeResult::default();
    collect_leaves(raw, &mut out, 0);
    out
}

#[cfg(test)]
mod tests;
