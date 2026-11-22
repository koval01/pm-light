//! Everything Blitz asks for while it lays out a page, answered here and nowhere else.
//!
//! A cleaned mail can name only pictures it carries itself, in `data:` addresses (`html::protect`). Those are
//! decoded here, within a budget of pixels: Blitz decodes a picture whole and keeps it, so a mail of sixteen
//! pictures of 4000 by 4000 pixels, a few kilobytes each, filled a gigabyte. Every other request is answered with
//! nothing: no connection is ever made, and an empty answer ends Blitz's wait for it.

use base64::{
    Engine as _,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Pixels one picture may have: 4000 by 4000.
pub const MOST_PIXELS_EACH: u64 = 16_000_000;
/// Pixels all the pictures of one page may have, about 160 MB once decoded.
pub const MOST_PIXELS: u64 = 40_000_000;

/// Data addresses are written by many programs, padded or not.
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// The bytes of a `data:` address: base64 when it says so, percent-encoded otherwise.
#[must_use]
pub fn decode(address: &str) -> Option<Vec<u8>> {
    let (scheme, rest) = address.split_at_checked(5)?;
    if !scheme.eq_ignore_ascii_case("data:") {
        return None;
    }
    let (head, body) = rest.split_once(',')?;
    let unescaped: Vec<u8> = percent_encoding::percent_decode_str(body).collect();
    let is_base64 = head.rsplit(';').next().is_some_and(|last| last.trim().eq_ignore_ascii_case("base64"));
    if !is_base64 {
        return Some(unescaped);
    }
    let packed: Vec<u8> = unescaped.into_iter().filter(|byte| !byte.is_ascii_whitespace()).collect();
    BASE64.decode(packed).ok()
}

/// The pixels a picture says it has, read from its header without decoding it. Nothing for what is no picture.
#[must_use]
pub fn pixels_of(bytes: &[u8]) -> Option<u64> {
    let size = imagesize::blob_size(bytes).ok()?;
    let (width, height) = (u64::try_from(size.width).ok()?, u64::try_from(size.height).ok()?);
    Some(width.saturating_mul(height))
}

/// The pictures of one page.
#[derive(Default)]
pub struct Pictures {
    pixels: Mutex<u64>,
    served: AtomicUsize,
    refused: AtomicUsize,
}

impl Pictures {
    /// Pictures handed to Blitz.
    pub fn served(&self) -> usize {
        self.served.load(Ordering::Relaxed)
    }

    /// Requests answered with nothing: addresses that are not the page's own, what is no picture, and pictures past
    /// the budget.
    pub fn refused(&self) -> usize {
        self.refused.load(Ordering::Relaxed)
    }

    /// The picture at `address`, if it is the page's own and fits what is left of the budget.
    fn take(&self, address: &str) -> Option<Vec<u8>> {
        let bytes = decode(address)?;
        let pixels = pixels_of(&bytes).filter(|pixels| *pixels <= MOST_PIXELS_EACH)?;
        let mut used = self.pixels.lock();
        let total = used.saturating_add(pixels);
        if total > MOST_PIXELS {
            return None;
        }
        *used = total;
        drop(used);
        Some(bytes)
    }
}

impl NetProvider for Pictures {
    fn fetch(&self, _document: usize, request: Request, handler: Box<dyn NetHandler>) {
        let address = request.url.as_str().to_string();
        let answer = (request.url.scheme() == "data").then(|| self.take(&address)).flatten();
        if answer.is_some() {
            self.served.fetch_add(1, Ordering::Relaxed);
        } else {
            self.refused.fetch_add(1, Ordering::Relaxed);
            log::debug!("the page asked for something it does not carry, and got nothing");
        }
        handler.bytes(address, Bytes::from(answer.unwrap_or_default()));
    }
}

#[cfg(test)]
mod tests;
