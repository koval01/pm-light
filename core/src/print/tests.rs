use super::*;
use crate::{
    crypto::SignatureStatus,
    details::MessageDetails,
    model::{Address, MailProtection, MessageMeta},
};

fn plain() -> PrintOptions {
    PrintOptions { headers: false, ..PrintOptions::default() }
}

fn labels() -> PrintLabels {
    PrintLabels {
        from: "From".into(),
        to: "To".into(),
        cc: "Cc".into(),
        date: "Date".into(),
        date_format: "%d.%m.%Y".into(),
        attachments: "Files".into(),
        encryption: "Encryption".into(),
        size: "Size".into(),
        message_id: "Id".into(),
        first_mail: "First mail".into(),
        earlier_mail: "{count} earlier".into(),
        test_title: "Test".into(),
        test_text: "Measure".into(),
        seal_title: "Seal".into(),
        seal_text: "Scan me".into(),
        seal_key: "Key".into(),
        seal_fingerprint: "Fingerprint".into(),
        seal_more: "More".into(),
    }
}

#[test]
fn a_mark_lands_where_din_5008_wants_it_on_paper() {
    for sheet in [87.0, 105.0, 148.5, 192.0, 210.0] {
        let on_paper = mark_top(sheet, 0.0).mul_add(MM_ON_PAPER, TOP_MARGIN_MM);
        assert!((on_paper - sheet).abs() < 0.001, "{sheet}");
    }
}

#[test]
fn the_shift_moves_marks_and_cannot_throw_them_off_the_sheet() {
    assert!((mark_top(105.0, 2.0) - mark_top(107.0, 0.0)).abs() < 0.001);
    assert_eq!(mark_top(105.0, 500.0), mark_top(105.0, MAX_SHIFT_MM));
}

#[test]
fn marks_follow_the_chosen_form() {
    assert!(marks(&plain()).is_empty());
    let form_b = marks(&PrintOptions { fold_marks: FoldMarks::B, ..plain() });
    assert_eq!(form_b.matches("class=\"mark\"").count(), 2);
    let with_hole = marks(&PrintOptions { fold_marks: FoldMarks::A, hole_mark: true, ..plain() });
    assert_eq!(with_hole.matches("class=\"mark\"").count(), 3);
}

#[test]
fn colours_follow_the_options() {
    assert!(!fit(&plain()).contains("grayscale"));
    let gray = PrintOptions { grayscale: true, ..plain() };
    assert!(fit(&gray).contains("img{filter:grayscale(1)"));
    assert!(!style(&gray).contains("filter"), "a filter over the page blurs its text");
    assert!(style(&plain()).contains("print-color-adjust:exact"));
    assert!(style(&PrintOptions { save_ink: true, ..plain() }).contains("print-color-adjust:economy"));
    assert!(fit(&PrintOptions { save_ink: true, ..plain() }).contains("background:none"));
}

#[test]
fn a_new_sheet_for_each_message_is_a_choice() {
    assert!(style(&plain()).contains("break-before:page"));
    assert!(style(&PrintOptions { new_sheet_each: false, ..plain() }).contains("break-before:auto"));
}

#[test]
fn an_address_without_a_name_is_just_the_address() {
    assert_eq!(address(&Address { name: None, email: "a@b.c".into() }), "a@b.c");
    let two = [Address { name: None, email: "a@b.c".into() }, Address { name: None, email: "d@e.f".into() }];
    assert_eq!(addresses(&two), "a@b.c, d@e.f");
}

#[test]
fn nothing_from_a_sender_stays_as_markup() {
    assert_eq!(row("<x>", "<img src=x>"), "<tr><th>&lt;x&gt;</th><td>&lt;img src=x&gt;</td></tr>");
}

#[test]
fn a_date_format_that_is_nonsense_cannot_panic() {
    assert!(!date_text(1_000_000, "%d.%m.%Y").is_empty());
    let _ = date_text(1_000_000, "%Q%");
    let _ = date_text(u64::MAX, "%d");
}

#[test]
fn the_test_sheet_carries_a_scale_and_the_chosen_marks() {
    let sheet = test_sheet(&PrintOptions { fold_marks: FoldMarks::B, ..plain() }, &labels());
    assert_eq!(sheet.parts.len(), 1);
    assert!(sheet.parts[0].marks.contains(">20<") && sheet.parts[0].marks.contains(">270<"));
    assert!(sheet.parts[0].html.contains("Measure"));
}

#[test]
fn history_names_a_first_mail_and_counts_the_rest() {
    let m = sample();
    let item = |earlier| PrintItem { message: &m, body: String::new(), earlier, seal: None };
    assert!(history_row(&item(None), &labels()).is_empty());
    assert!(history_row(&item(Some(0)), &labels()).contains("First mail"));
    assert!(history_row(&item(Some(7)), &labels()).contains("7 earlier"));
}

#[test]
fn a_seal_gets_a_sheet_of_its_own_that_says_how_to_check_it() {
    let m = sample();
    let sheet = SealSheet {
        seal: Seal { statement: "signed".into(), text_codes: vec!["PMS1.a.0.1.x".into()] },
        signer: "me+x@proton.me".into(),
        fingerprint: "abc123".into(),
    };
    let items = [PrintItem { message: &m, body: String::new(), earlier: None, seal: Some(sheet) }];
    let html = &document(&items, &plain(), &labels()).parts[0].html;
    assert!(html.contains(r#"class="seal-sheet""#) && html.contains("Scan me"));
    assert!(html.contains("search=me%2Bx%40proton.me") && html.contains("abc123"));
    assert_eq!(html.matches("<svg").count(), 3, "the signed code, its copy and the text");
}

#[test]
fn no_seal_means_no_sheet() {
    assert!(seal_sheet(None, &labels()).is_empty());
}

#[test]
fn only_the_chosen_blocks_are_printed() {
    let m = sample();
    let item = PrintItem { message: &m, body: "<p>x</p>".into(), earlier: None, seal: None };
    let bare = heading(&item, &plain(), &labels());
    assert!(bare.contains("<h1>Subject</h1>") && !bare.contains("<table>"));
    let full = heading(&item, &PrintOptions { headers: true, technical: true, ..plain() }, &labels());
    assert!(full.contains("From") && full.contains("Encryption") && full.contains("&lt;id@x&gt;"));
}

#[test]
fn marks_go_on_every_message_only_when_each_starts_a_sheet() {
    let m = sample();
    let items = [
        PrintItem { message: &m, body: String::new(), earlier: None, seal: None },
        PrintItem { message: &m, body: String::new(), earlier: None, seal: None },
    ];
    let joined = PrintOptions { fold_marks: FoldMarks::B, new_sheet_each: false, ..plain() };
    let parts = document(&items, &joined, &labels()).parts;
    assert!(!parts[0].marks.is_empty() && parts[1].marks.is_empty());
    let apart = PrintOptions { fold_marks: FoldMarks::B, ..plain() };
    assert!(document(&items, &apart, &labels()).parts.iter().all(|part| !part.marks.is_empty()));
}

#[test]
fn a_fingerprint_changes_with_anything_that_matters() {
    let original = fingerprint(&sample());
    assert_eq!(fingerprint(&sample()), original);
    let changed = |edit: fn(&mut Message)| {
        let mut m = sample();
        edit(&mut m);
        fingerprint(&m)
    };
    assert_ne!(changed(|m| m.body.push('x')), original);
    assert_ne!(changed(|m| m.meta.subject.push('x')), original);
    assert_ne!(changed(|m| m.meta.date += 1), original);
    assert_ne!(changed(|m| m.meta.from.email.push('x')), original);
    assert_ne!(changed(|m| m.cc.push(Address { name: None, email: "x@y.z".into() })), original);
}

#[test]
fn text_moved_between_fields_is_a_different_message() {
    let mut a = sample();
    let mut b = sample();
    (a.meta.subject, a.body) = ("ab".into(), "c".into());
    (b.meta.subject, b.body) = ("a".into(), "bc".into());
    assert_ne!(fingerprint(&a), fingerprint(&b));
}

#[test]
fn a_check_code_reads_back() {
    let m = sample();
    let code = check_code(&m);
    assert_eq!(parse_check_code(&format!("  {code}\n")), Some(("1", fingerprint(&m).as_str())));
}

#[test]
fn only_our_codes_are_read_and_their_ids_cannot_reach_another_path() {
    for code in ["", "x", "PML1", "PML1..f", "PML2.id.f", "PML1.id.", "PML1.id.f.extra", "PML1.../../x.f", "PML1.a?b.f", "PML1.a b.f"] {
        assert_eq!(parse_check_code(code), None, "{code}");
    }
    assert_eq!(parse_check_code(&format!("PML1.{}.f", "a".repeat(LONGEST_ID + 1))), None);
}

#[test]
fn the_code_is_printed_only_when_asked() {
    let m = sample();
    let item = PrintItem { message: &m, body: String::new(), earlier: None, seal: None };
    assert!(!heading(&item, &plain(), &labels()).contains("<svg"));
    let coded = heading(&item, &PrintOptions { check_code: true, ..plain() }, &labels());
    assert!(coded.contains("<svg") && !coded.contains("<?xml"));
}

fn sample() -> Message {
    let from = Address { name: Some("<b>Ann</b>".into()), email: "a@b.c".into() };
    Message {
        meta: MessageMeta {
            id: "1".into(),
            folder: "inbox".into(),
            from,
            to: Vec::new(),
            subject: "Subject".into(),
            snippet: String::new(),
            date: 0,
            unread: false,
            starred: false,
            labels: Vec::new(),
            attachment_count: 0,
            conversation_id: "c".into(),
            messages: 0,
            expires: 0,
        },
        address_id: String::new(),
        size: 10,
        signature: SignatureStatus::Unsigned,
        raw_headers: String::new(),
        unsubscribe: None,
        receipt_requested: false,
        cc: Vec::new(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        body: String::new(),
        safe_body: String::new(),
        protection: MailProtection::default(),
        details: MessageDetails { encryption: "end-to-end".into(), auth: Vec::new(), message_id: "<id@x>".into() },
        html: false,
        attachments: Vec::new(),
        from_proton: Some(false),
    }
}

#[test]
fn a_code_on_screen_has_its_quiet_zone_and_dark_modules() {
    let (pixels, side) = qr_pixels("otpauth://totp/a?secret=B", 4).unwrap();
    assert_eq!(pixels.len(), (side * side * 4) as usize);
    assert_eq!(&pixels[..4], &[255, 255, 255, 255]);
    assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[0] == 0));
    assert!(pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
}
