use super::*;

#[test]
fn web_and_mail_addresses_may_leave_the_app() {
    for link in ["https://proton.me/", "http://example.com/a?b=c", "mailto:a@example.com", "HTTPS://proton.me/"] {
        assert!(openable(link).is_some(), "{link}");
    }
}

#[test]
fn nothing_that_could_run_something_leaves_the_app() {
    for link in [
        "file:///etc/passwd",
        "smb://evil.example/share",
        "javascript:alert(1)",
        "x-apple.systempreferences:com.apple.preference",
        "ms-msdt:/id",
        "",
        "not a url",
    ] {
        assert!(openable(link).is_none(), "{link}");
    }
}

#[test]
fn office_files_with_macros_are_known_by_name() {
    assert!(may_hold_macros("budget.XLSM") && may_hold_macros("letter.docm"));
    assert!(!may_hold_macros("budget.xlsx") && !may_hold_macros("letter.docx") && !may_hold_macros("docm"));
}

#[test]
fn only_safe_types_are_shown() {
    assert!(can_preview("IMAGE/PNG") && can_preview("text/plain"));
    assert!(!can_preview("text/html") && !can_preview("application/pdf") && !can_preview(""));
}

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8,
    6, 0, 0, 0,
];

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut data = PNG_1X1.to_vec();
    data[16..20].copy_from_slice(&width.to_be_bytes());
    data[20..24].copy_from_slice(&height.to_be_bytes());
    data
}

const WINDOWS_PROGRAM: &[u8] = b"MZ\x90\x00\x03\x00\x00\x00";
const MAC_PROGRAM: &[u8] = &[0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 1];

#[test]
fn ordinary_documents_raise_nothing() {
    assert_eq!(assess("report.pdf", b"%PDF-1.7"), None);
    assert_eq!(assess("notes", b"hello"), None);
}

#[test]
fn what_runs_on_opening_is_flagged_in_any_case() {
    for name in ["a.WEBLOC", "a.mobileconfig", "a.docm", "a.html", "a.terminal", "a.desktop", "a.py"] {
        assert_eq!(assess(name, b"hello"), Some(FileRisk::Runnable), "{name}");
    }
}

#[test]
fn a_program_by_name_is_flagged() {
    for name in ["setup.exe", "invoice.pdf.exe", "RUN.BAT", "x.ps1", "a.command", "b.scr"] {
        assert_eq!(assess(name, b"hello"), Some(FileRisk::Runnable), "{name}");
    }
}

#[test]
fn a_program_dressed_as_a_document_is_called_out() {
    assert_eq!(assess("invoice.pdf", WINDOWS_PROGRAM), Some(FileRisk::Disguised));
    assert_eq!(assess("photo.jpg", MAC_PROGRAM), Some(FileRisk::Disguised));
}

#[test]
fn an_honest_program_is_only_runnable() {
    assert_eq!(assess("tool.exe", WINDOWS_PROGRAM), Some(FileRisk::Runnable));
}

#[test]
fn a_real_image_passes() {
    assert!(is_displayable_image("image/png", &png_header(800, 600)));
}

#[test]
fn a_decompression_bomb_is_refused_before_decoding() {
    assert!(!is_displayable_image("image/png", &png_header(100_000, 100_000)));
}

#[test]
fn something_that_only_claims_to_be_an_image_is_refused() {
    assert!(!is_displayable_image("image/png", b"<script>alert(1)</script>"));
}

#[test]
fn an_oversized_image_is_refused_unread() {
    assert!(!is_displayable_image("image/png", &vec![0; MAX_IMAGE_BYTES + 1]));
}

#[test]
fn an_svg_has_its_own_small_limit() {
    assert!(is_displayable_image("image/svg+xml", b"<svg/>"));
    assert!(!is_displayable_image("image/svg+xml", &vec![b' '; MAX_SVG_BYTES + 1]));
}
