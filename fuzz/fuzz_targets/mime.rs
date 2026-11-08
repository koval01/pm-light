//! A raw message as it comes from a sender: MIME parts, encodings and headers.
#![no_main]

use libfuzzer_sys::fuzz_target;
use proton_core::mime::{decode_encoded_words, parse_mime};

fuzz_target!(|raw: &[u8]| {
    let _ = parse_mime(raw);
    if let Ok(header) = std::str::from_utf8(raw) {
        let _ = decode_encoded_words(header);
    }
});
