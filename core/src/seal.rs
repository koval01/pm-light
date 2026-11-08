//! A signed copy of a message on paper, in a standard anyone can check.
//!
//! The first code holds an OpenPGP clear-signed statement (RFC 9580), which needs no part of this app: any
//! OpenPGP tool with the signer's public key verifies it, and a phone's camera shows it as plain text. A longer
//! mail goes in further codes, tied to the signature by the hash in that statement, with spare codes that stand in
//! for any that are lost.
//!
//! It works as a visible digital seal does: the data is in the code, and whoever checks a sheet
//! compares it with the paper. It is not one of those (ICAO Doc 9303-13, BSI TR-03137), which name
//! an issuing state and rest on its certificates. A seal here says which address printed the mail
//! and what it said then. It says nothing about the paper beside it.

use crate::error::{Error, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use flate2::{Compression, read::DeflateDecoder, write::DeflateEncoder};
use reed_solomon_erasure::galois_8::ReedSolomon;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::{Read, Write},
};

const TITLE: &str = "Printed mail, sealed by Proton Mail Light";
const SIGNED_START: &str = "-----BEGIN PGP SIGNED MESSAGE-----";
const SIGNED_END: &str = "-----END PGP SIGNATURE-----";
const TEXT_PREFIX: &str = "PMS1";
/// Little enough that the signed code stays large on paper, where wear and stains cost it least.
const MOST_INLINE_BYTES: usize = 300;
const MOST_FIELD_CHARS: usize = 200;
/// As text in a code this takes 480 bytes, which prints at nearly a millimetre a module.
const SHARD_BYTES: usize = 360;
pub const MOST_CODES: usize = 48;
/// One spare code for every this many of text.
const SPARE_EVERY: usize = 3;
/// Codes are hostile input: what they inflate to is bounded before it is read.
const MOST_INFLATED: u64 = 512 * 1024;
const ID_CHARS: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedMail {
    pub signer: String,
    pub sealed_at: u64,
    pub sender: String,
    pub recipients: Vec<String>,
    pub date: u64,
    pub subject: String,
    pub text: String,
    pub attachments: Vec<String>,
}

/// What goes on paper. The first code stands alone, and the others only add the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seal {
    pub statement: String,
    pub text_codes: Vec<String>,
}

/// A signature covers lines without their trailing spaces, so that is the text that is hashed too.
fn normal_text(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.join("\n").trim_end().to_string()
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MOST_FIELD_CHARS).collect()
}

fn text_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn time_text(milliseconds: u64) -> String {
    let seconds = i64::try_from(milliseconds / 1000).unwrap_or(0);
    DateTime::<Utc>::from_timestamp(seconds, 0).map_or_else(String::new, |time| time.to_rfc3339())
}

fn time_of(text: &str) -> u64 {
    let parsed = DateTime::parse_from_rfc3339(text).ok();
    parsed.and_then(|time| u64::try_from(time.timestamp()).ok()).map_or(0, |seconds| seconds * 1000)
}

/// No line may end in a space: OpenPGP tools drop those before checking a clear signature, so one
/// left in makes a genuine seal fail everywhere but here. GnuPG found this.
fn statement(mail: &SealedMail, text: &str) -> String {
    let fields = [
        ("Signer", mail.signer.clone()),
        ("Printed", time_text(mail.sealed_at)),
        ("From", mail.sender.clone()),
        ("To", mail.recipients.join(", ")),
        ("Date", time_text(mail.date)),
        ("Subject", mail.subject.clone()),
        ("Files", mail.attachments.join(", ")),
        ("Text-SHA256", text_hash(text)),
    ];
    let mut out = format!("{TITLE}\n");
    for (name, value) in fields {
        let value = one_line(&value);
        if !value.is_empty() {
            let _ = writeln!(out, "{name}: {value}");
        }
    }
    if text.len() <= MOST_INLINE_BYTES {
        out.push('\n');
        out.push_str(text);
    }
    out
}

fn deflate(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(bytes).map_err(Error::other_from)?;
    encoder.finish().map_err(Error::other_from)
}

fn inflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    DeflateDecoder::new(bytes).take(MOST_INFLATED + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= MOST_INFLATED).then_some(out)
}

const fn spares_for(parts: usize) -> usize {
    if parts < 2 { 0 } else { parts.div_ceil(SPARE_EVERY) }
}

/// Spare codes worked out from the parts, so that any of them may be lost, whichever they are: to
/// a stain, a torn corner, or a streak down a whole column.
fn spare_shards(chunks: &[&[u8]]) -> Result<Vec<Vec<u8>>> {
    let spares = spares_for(chunks.len());
    let coder = ReedSolomon::new(chunks.len(), spares).map_err(Error::other_from)?;
    let mut shards: Vec<Vec<u8>> = chunks
        .iter()
        .map(|chunk| {
            let mut shard = chunk.to_vec();
            shard.resize(SHARD_BYTES, 0);
            shard
        })
        .collect();
    shards.resize(chunks.len() + spares, vec![0; SHARD_BYTES]);
    coder.encode(&mut shards).map_err(Error::other_from)?;
    Ok(shards.split_off(chunks.len()))
}

fn text_codes(text: &str) -> Result<Vec<String>> {
    if text.len() <= MOST_INLINE_BYTES {
        return Ok(Vec::new());
    }
    let hash = text_hash(text);
    let id: String = hash.chars().take(ID_CHARS).collect();
    let packed = deflate(text.as_bytes())?;
    let chunks: Vec<&[u8]> = packed.chunks(SHARD_BYTES).collect();
    if chunks.len() > MOST_CODES {
        return Err(Error::TooLongToSeal);
    }
    let total = chunks.len();
    let spares = if total < 2 { Vec::new() } else { spare_shards(&chunks)? };
    let parts = chunks.iter().copied().chain(spares.iter().map(Vec::as_slice));
    Ok(parts
        .enumerate()
        .map(|(index, part)| format!("{TEXT_PREFIX}.{id}.{index}.{total}.{}", URL_SAFE_NO_PAD.encode(part)))
        .collect())
}

/// `sign` makes the OpenPGP clear signature, over text anyone can read.
pub fn pack(mail: &SealedMail, sign: impl FnOnce(&str) -> Result<String>) -> Result<Seal> {
    let text = normal_text(&mail.text);
    let text_codes = text_codes(&text)?;
    Ok(Seal { statement: sign(&statement(mail, &text))?, text_codes })
}

#[must_use]
pub fn is_text_code(code: &str) -> bool {
    code.starts_with(TEXT_PREFIX)
}

/// Who claims to have signed, read before the signature is checked: it only says whose key to fetch.
#[must_use]
pub fn claimed_signer(codes: &[String]) -> Option<String> {
    let armored = signed_code(codes)?;
    armored.lines().find_map(|line| line.strip_prefix("Signer: ").map(str::trim).map(str::to_string))
}

/// Separates text pasted from a phone: a signed message spans many lines, a text code just one.
pub fn pasted_codes(text: &str) -> Vec<String> {
    let mut codes = Vec::new();
    let rest = match text.split_once(SIGNED_START) {
        Some((before, signed)) => {
            let (block, after) = signed.split_once(SIGNED_END).unwrap_or((signed, ""));
            codes.push(format!("{SIGNED_START}{block}{SIGNED_END}"));
            codes.extend(before.split_whitespace().map(str::to_string));
            after
        }
        None => text,
    };
    codes.extend(rest.split_whitespace().map(str::to_string));
    codes
}

#[must_use]
pub fn signed_code(codes: &[String]) -> Option<&str> {
    codes.iter().map(|code| code.trim_start()).find(|code| code.starts_with(SIGNED_START))
}

struct Part<'a> {
    id: &'a str,
    index: usize,
    total: usize,
    data: &'a str,
}

fn read_part(code: &str) -> Option<Part<'_>> {
    let mut fields = code.trim().splitn(5, '.');
    if fields.next()? != TEXT_PREFIX {
        return None;
    }
    let id = fields.next()?;
    let index: usize = fields.next()?.parse().ok()?;
    let total: usize = fields.next()?.parse().ok()?;
    let data = fields.next()?;
    (index < total + spares_for(total) && total <= MOST_CODES).then_some(Part { id, index, total, data })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Assembly {
    Incomplete { have: usize, need: usize },
    /// The signature holds, and what came with it does not match what it signed.
    Damaged,
    Complete(Box<SealedMail>),
}

struct Statement<'a> {
    fields: BTreeMap<&'a str, &'a str>,
    text: Option<&'a str>,
}

fn read_statement(verified: &str) -> Option<Statement<'_>> {
    let (head, text) = verified.split_once("\n\n").map_or_else(|| (verified.trim_end(), None), |(h, t)| (h, Some(t)));
    let mut lines = head.lines();
    if lines.next()?.trim_end() != TITLE {
        return None;
    }
    let fields = lines.filter_map(|line| line.split_once(": ")).collect();
    Some(Statement { fields, text })
}

fn list(field: &str) -> Vec<String> {
    field.split(", ").filter(|item| !item.is_empty()).map(str::to_string).collect()
}

fn text_from_codes(codes: &[String], wanted: &str) -> std::result::Result<String, Assembly> {
    let id: String = wanted.chars().take(ID_CHARS).collect();
    let parts: Vec<Part<'_>> = codes.iter().filter_map(|code| read_part(code)).filter(|p| p.id == id).collect();
    let need = parts.first().map_or(1, |part| part.total);
    let spares = spares_for(need);
    let mut found: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for part in parts.iter().filter(|part| part.total == need) {
        found.insert(part.index, URL_SAFE_NO_PAD.decode(part.data).map_err(|_| Assembly::Damaged)?);
    }
    let have = found.len();
    if have < need {
        return Err(Assembly::Incomplete { have, need });
    }
    let packed: Vec<u8> = if (0..need).all(|index| found.contains_key(&index)) {
        found.range(..need).flat_map(|(_, part)| part.iter().copied()).collect()
    } else {
        let coder = ReedSolomon::new(need, spares).map_err(|_| Assembly::Damaged)?;
        let mut shards: Vec<Option<Vec<u8>>> = (0..need + spares)
            .map(|index| {
                found.get(&index).map(|part| {
                    let mut shard = part.clone();
                    shard.resize(SHARD_BYTES, 0);
                    shard
                })
            })
            .collect();
        coder.reconstruct_data(&mut shards).map_err(|_| Assembly::Damaged)?;
        shards.into_iter().take(need).flatten().flatten().collect()
    };
    let text = inflate(&packed).and_then(|bytes| String::from_utf8(bytes).ok());
    text.ok_or(Assembly::Damaged)

}

/// Builds the mail from a statement whose signature held, and the codes read with it.
#[must_use]
pub fn assemble(verified: &str, codes: &[String]) -> Assembly {
    let Some(statement) = read_statement(verified) else { return Assembly::Damaged };
    let field = |name: &str| statement.fields.get(name).copied().unwrap_or_default();
    let wanted = field("Text-SHA256");
    let text = match statement.text {
        Some(text) => normal_text(text),
        None => match text_from_codes(codes, wanted) {
            Ok(text) => text,
            Err(problem) => return problem,
        },
    };
    if text_hash(&text) != wanted {
        return Assembly::Damaged;
    }
    Assembly::Complete(Box::new(SealedMail {
        signer: field("Signer").to_string(),
        sealed_at: time_of(field("Printed")),
        sender: field("From").to_string(),
        recipients: list(field("To")),
        date: time_of(field("Date")),
        subject: field("Subject").to_string(),
        text,
        attachments: list(field("Files")),
    }))
}

#[cfg(test)]
mod tests;
