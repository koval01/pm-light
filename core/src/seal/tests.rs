use super::*;

fn mail(text: &str) -> SealedMail {
    SealedMail {
        signer: "me@proton.me".into(),
        sealed_at: 1_700_000_000_000,
        sender: "a@b.c".into(),
        recipients: vec!["me@proton.me".into(), "x@y.z".into()],
        date: 1_690_000_000_000,
        subject: "Subject".into(),
        text: text.into(),
        attachments: vec!["file.pdf".into()],
    }
}

/// A signer that stands in for OpenPGP: the signed text is what comes back once a signature has held.
fn keep() -> impl FnOnce(&str) -> Result<String> {
    |text| Ok(text.to_string())
}

fn noise(length: usize) -> String {
    let mut state = 1_u32;
    (0..length)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            char::from(b'!' + u8::try_from(state >> 24).unwrap() % 90)
        })
        .collect()
}

#[test]
fn a_short_mail_is_whole_in_the_signed_code() {
    let seal = pack(&mail("Hello\nthere"), keep()).unwrap();
    assert!(seal.text_codes.is_empty());
    assert!(seal.statement.contains("Hello\nthere") && seal.statement.contains("From: a@b.c"));
    assert_eq!(assemble(&seal.statement, &[]), Assembly::Complete(Box::new(mail("Hello\nthere"))));
}

#[test]
fn the_signed_part_is_readable_by_a_person() {
    let seal = pack(&mail("Hello"), keep()).unwrap();
    let first = seal.statement.lines().next().unwrap();
    assert_eq!(first, TITLE);
    assert!(seal.statement.contains("Subject: Subject") && seal.statement.contains("Text-SHA256: "));
}

#[test]
fn a_long_mail_goes_in_further_codes_held_by_the_hash() {
    let long = noise(4000);
    let seal = pack(&mail(&long), keep()).unwrap();
    assert!(seal.text_codes.len() > 2 && !seal.statement.contains(&long));
    let shuffled: Vec<String> = seal.text_codes.iter().rev().cloned().collect();
    let Assembly::Complete(found) = assemble(&seal.statement, &shuffled) else { panic!("not complete") };
    assert_eq!(found.text, long);
}

#[test]
fn a_missing_code_is_counted_and_the_signed_part_still_stands() {
    let seal = pack(&mail(&noise(4000)), keep()).unwrap();
    let parts: usize = seal.text_codes[0].split('.').nth(3).unwrap().parse().unwrap();
    let left: Vec<String> = seal.text_codes[seal.text_codes.len() - parts + 1..].to_vec();
    assert_eq!(assemble(&seal.statement, &left), Assembly::Incomplete { have: parts - 1, need: parts });
    assert_eq!(assemble(&seal.statement, &[]), Assembly::Incomplete { have: 0, need: 1 });
}

#[test]
fn text_swapped_under_a_good_signature_is_found() {
    let seal = pack(&mail(&noise(4000)), keep()).unwrap();
    let other = pack(&mail(&noise(4100)), keep()).unwrap();
    assert!(matches!(assemble(&seal.statement, &other.text_codes), Assembly::Incomplete { .. }));
    let id: String = seal.text_codes[0].split('.').nth(1).unwrap().to_string();
    let forged: Vec<String> = other
        .text_codes
        .iter()
        .map(|code| {
            let mut fields: Vec<&str> = code.split('.').collect();
            fields[1] = &id;
            fields.join(".")
        })
        .collect();
    assert_eq!(assemble(&seal.statement, &forged), Assembly::Damaged);
}

#[test]
fn an_inline_text_changed_after_signing_is_found() {
    let seal = pack(&mail("Pay 100"), keep()).unwrap();
    assert_eq!(assemble(&seal.statement.replace("Pay 100", "Pay 900"), &[]), Assembly::Damaged);
}

#[test]
fn a_subject_cannot_write_fields_of_its_own() {
    let hostile = SealedMail { subject: "x\nSigner: evil@x.y\n\nFake".into(), ..mail("Hello") };
    let seal = pack(&hostile, keep()).unwrap();
    let Assembly::Complete(found) = assemble(&seal.statement, &[]) else { panic!("not complete") };
    assert_eq!(found.signer, "me@proton.me");
    assert_eq!(found.text, "Hello");
}

#[test]
fn no_signed_line_ends_in_a_space_whatever_is_empty() {
    let bare = SealedMail { attachments: Vec::new(), recipients: Vec::new(), subject: "  ".into(), ..mail("a  \nb ") };
    let seal = pack(&bare, keep()).unwrap();
    assert!(seal.statement.lines().all(|line| line == line.trim_end()), "{}", seal.statement);
    assert!(matches!(assemble(&seal.statement, &[]), Assembly::Complete(_)));
}

#[test]
fn trailing_spaces_do_not_break_the_hash() {
    let seal = pack(&mail("line   \nnext\r\nlast\n\n"), keep()).unwrap();
    let Assembly::Complete(found) = assemble(&seal.statement, &[]) else { panic!("not complete") };
    assert_eq!(found.text, "line\nnext\nlast");
}

#[test]
fn a_mail_too_long_for_the_codes_is_refused_and_not_cut() {
    assert!(matches!(pack(&mail(&noise(100_000)), keep()), Err(Error::TooLongToSeal)));
}

#[test]
fn the_signer_is_found_only_in_a_signed_message() {
    assert_eq!(claimed_signer(&["PMS1.a.0.1.x".to_string()]), None);
    let code = format!("{SIGNED_START}\nHash: SHA512\n\n{TITLE}\nSigner: me@proton.me\n");
    assert_eq!(claimed_signer(&[code]).as_deref(), Some("me@proton.me"));
}

#[test]
fn pasted_text_keeps_a_signed_message_whole() {
    let signed = format!("{SIGNED_START}\nHash: SHA512\n\nA line\n{SIGNED_END}");
    let pasted = format!("PMS1.a.0.2.x\n{signed}\nPMS1.a.1.2.y\n");
    let codes = pasted_codes(&pasted);
    assert_eq!(codes.len(), 3);
    assert_eq!(signed_code(&codes), Some(signed.as_str()));
    assert_eq!(pasted_codes("PML1.a.b\n"), ["PML1.a.b"]);
    assert!(pasted_codes("  ").is_empty());
}

#[test]
fn something_else_signed_is_not_a_seal() {
    assert_eq!(assemble("Just some text someone signed", &[]), Assembly::Damaged);
}

#[test]
fn a_decompression_bomb_stops_at_the_limit() {
    let bomb = deflate(&vec![0_u8; 8 * 1024 * 1024]).unwrap();
    assert_eq!(inflate(&bomb), None);
}

#[test]
fn junk_codes_cannot_panic_the_reader() {
    for code in ["", "PMS1", "PMS1.a.b.c.d", "PMS1.a.5.2.x", "PMS1.a.0.999.x", "PMS1.a.0.1.!!!"] {
        let _ = text_from_codes(&[code.to_string()], "abcdefabcdefabcdef");
    }
    let code = format!("PMS1.aaaaaaaaaaaa.0.1.{}", URL_SAFE_NO_PAD.encode([9_u8; 40]));
    assert!(matches!(text_from_codes(&[code], "aaaaaaaaaaaa"), Err(Assembly::Damaged)));
}

#[test]
fn any_codes_up_to_the_number_of_spares_can_be_lost() {
    let seal = pack(&mail(&noise(4000)), keep()).unwrap();
    let all = seal.text_codes.len();
    let need = all - spares_for(all - all / 4);
    let spares = all - need;
    assert!(spares >= 2, "{all} codes");
    for first in 0..all {
        let lost: Vec<usize> = (0..spares).map(|step| (first + step * 2) % all).collect();
        let codes: Vec<String> = seal
            .text_codes
            .iter()
            .enumerate()
            .filter(|(index, _)| !lost.contains(index))
            .map(|(_, code)| code.clone())
            .collect();
        let Assembly::Complete(found) = assemble(&seal.statement, &codes) else { panic!("{lost:?} lost") };
        assert_eq!(found.text, noise(4000));
    }
}

#[test]
fn sheets_printed_before_spare_codes_still_read() {
    let packed = URL_SAFE_NO_PAD.encode(deflate(noise(1500).as_bytes()).unwrap());
    let hash = text_hash(&noise(1500));
    let id: String = hash.chars().take(ID_CHARS).collect();
    let chunks: Vec<String> = packed.as_bytes().chunks(900).map(|c| String::from_utf8(c.to_vec()).unwrap()).collect();
    let total = chunks.len();
    let codes: Vec<String> =
        chunks.iter().enumerate().map(|(i, chunk)| format!("{TEXT_PREFIX}.{id}.{i}.{total}.{chunk}")).collect();
    let seal = pack(&mail(&noise(1500)), keep()).unwrap();
    assert!(matches!(assemble(&seal.statement, &codes), Assembly::Complete(_)));
}
