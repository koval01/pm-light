//! Codes pasted or scanned off a printed sheet, and the check code beside a printed mail.
#![no_main]

use libfuzzer_sys::fuzz_target;
use proton_core::{print::parse_check_code, seal};

fuzz_target!(|input: (&str, &str)| {
    let (pasted, verified) = input;
    let codes = seal::pasted_codes(pasted);
    let _ = seal::is_text_code(pasted);
    let _ = seal::claimed_signer(&codes);
    let _ = seal::signed_code(&codes);
    let _ = seal::assemble(verified, &codes);
    let _ = parse_check_code(pasted);
});
