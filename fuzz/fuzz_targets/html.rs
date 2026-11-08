//! A mail's HTML and CSS, cleaned for the reader and for the editor.
#![no_main]

use libfuzzer_sys::fuzz_target;
use proton_core::html::{clean_css, clean_signature, for_editor, protect, without_embedded};

fuzz_target!(|html: &str| {
    let _ = protect(html);
    let _ = for_editor(html);
    let _ = clean_signature(html);
    let _ = clean_css(html);
    let _ = without_embedded(html);
});
