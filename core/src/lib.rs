//! Everything behind the interface: the network, sign-in, keys and mail. It knows nothing about
//! windows, so all of it can be tested without one.

pub mod calendar;
pub mod client;
pub mod compose;
pub mod crypto;
pub mod details;
pub mod dns;
pub mod domain_check;
pub mod eml;
pub mod error;
pub mod file_kind;
pub mod html;
pub mod image_metadata;
pub mod journal;
pub mod keymap;
pub mod mailto;
pub mod mapping;
pub mod mime;
mod pattern;
pub mod model;
pub mod print;
pub mod ranking;
pub mod safety;
pub mod mail_store;
pub mod scan;
pub mod search;
pub mod seal;
pub mod send;
pub mod service;
pub mod sieve;
#[cfg(test)]
mod test_server;
pub mod text;
pub mod timing;
pub mod tls;
pub mod tor;
pub mod vault;
pub mod vcard;
pub mod view;

pub use client::{Client, Ending, Session, PROTON_API};

/// Where the code lives, and where a build's commit can be read.
pub const REPOSITORY: &str = "https://github.com/koval01/pm-light";
/// The most a Windows credential holds, `CRED_MAX_CREDENTIAL_BLOB_SIZE`. Text goes there as UTF-16,
/// so a saved session must stay under half as many characters.
pub const WINDOWS_CREDENTIAL_BYTES: usize = 2560;

/// Bytes as lowercase hexadecimal, two digits each.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .filter_map(|digit| char::from_digit(u32::from(digit), 16))
        .collect()
}
pub use error::{Error, Link, Result};
pub use service::{AuthStep, PrintCheck, PrintScope, SealCheck, Service, SessionStore, SheetReport};
