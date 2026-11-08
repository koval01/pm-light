//! What a person types into the search field, pasted text included.
#![no_main]

use libfuzzer_sys::fuzz_target;
use proton_core::search::{begin_and_end, hints, parse};

fuzz_target!(|input: &str| {
    let parsed = parse(input);
    let _ = begin_and_end(&parsed.query);
    let _ = hints(input);
});
