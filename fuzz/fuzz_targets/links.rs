//! Links from a mail or another program: web addresses to clean, and `mailto:` links to open a message from.
#![no_main]

use libfuzzer_sys::fuzz_target;
use proton_core::{html::clean_link, mailto};

fuzz_target!(|link: &str| {
    let _ = clean_link(link);
    let _ = mailto::parse(link);
});
