//! An attachment: a stranger's name, type and bytes, examined for what it really is and stripped of metadata.
#![no_main]

use libfuzzer_sys::fuzz_target;
use proton_core::{file_kind, image_metadata::without_metadata};

fuzz_target!(|input: (&str, &str, &[u8])| {
    let (name, mime, data) = input;
    let _ = file_kind::examine(name, mime, data);
    let _ = without_metadata(data.to_vec());
});
