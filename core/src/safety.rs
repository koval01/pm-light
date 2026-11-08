//! What can be said about a file without trusting its name or its declared type.

use serde::Serialize;
use url::Url;

/// Past these, a picture is there to exhaust the machine that decodes it.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const MAX_IMAGE_PIXELS: u64 = 50_000_000;
const MAX_SVG_BYTES: usize = 1024 * 1024;

/// Run or interpreted when opened, whatever else the name says.
const RUNNABLE: &[&str] = &[
    // Windows
    "appinstaller", "appx", "appxbundle", "bat", "chm", "cmd", "com", "cpl", "dll", "exe", "gadget", "hlp", "hta",
    "img", "inf", "ins", "iso", "jar", "jnlp", "js", "jse", "library-ms", "lnk", "mcf", "msc", "msi", "msix",
    "msixbundle", "msp", "mst", "pif", "ps1", "psm1", "reg", "scf", "scr", "search-ms", "settingcontent-ms",
    "url", "vb", "vbe", "vbs", "vhd", "vhdx", "ws", "wsc", "wsf", "wsh",
    // macOS
    "action", "app", "applescript", "command", "dmg", "fileloc", "inetloc", "kext", "mobileconfig", "mpkg",
    "osax", "pkg", "prefpane", "scpt", "scptd", "terminal", "tool", "webloc", "workflow",
    // Linux and scripts anywhere
    "appimage", "bash", "deb", "desktop", "fish", "pl", "py", "rb", "rpm", "run", "sh", "zsh",
    // Macros
    "docm", "dotm", "potm", "pptm", "xlam", "xlsm", "xltm",
    // Pages that open in a browser and imitate a sign-in, with no sanitizer between them and the reader
    "htm", "html", "mht", "mhtml", "shtml", "svg", "xht", "xhtml",
];

const OPENABLE: &[&str] = &["http", "https", "mailto"];
const MACRO_DOCUMENTS: &[&str] =
    &["docm", "dotm", "potm", "ppam", "ppsm", "pptm", "sldm", "xlam", "xlsb", "xlsm", "xltm"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileRisk {
    /// A program or script by its name.
    Runnable,
    /// A program by its content, whatever its name claims.
    Disguised,
}

fn extension(name: &str) -> String {
    name.rsplit('.').next().filter(|e| e.len() < name.len()).unwrap_or_default().to_ascii_lowercase()
}

/// Not antivirus: it cannot tell a good program from a bad one, only that this is a program.
/// Office files of the kinds that can carry macros, which run once such a file is opened and allowed.
#[must_use]
pub fn may_hold_macros(name: &str) -> bool {
    MACRO_DOCUMENTS.contains(&extension(name).as_str())
}

/// Mail is hostile input.
///
/// Whatever a message links to, only web and mail addresses leave the app: `file:`, `smb:` or a custom scheme could
/// start a program on this machine. The address comes back as parsed, so that what was checked is exactly what the
/// system gets.
#[must_use]
pub fn openable(link: &str) -> Option<Url> {
    Url::parse(link).ok().filter(|parsed| OPENABLE.contains(&parsed.scheme()))
}

#[must_use]
pub fn is_runnable(name: &str) -> bool {
    RUNNABLE.contains(&extension(name).as_str())
}

#[must_use]
pub fn assess(name: &str, data: &[u8]) -> Option<FileRisk> {
    let runs_by_name = is_runnable(name);
    let runs_by_content = infer::get(data).is_some_and(|kind| kind.matcher_type() == infer::MatcherType::App);
    match (runs_by_name, runs_by_content) {
        (_, true) if !runs_by_name => Some(FileRisk::Disguised),
        (true, _) | (_, true) => Some(FileRisk::Runnable),
        _ => None,
    }
}

/// Checks the bytes, not the sender's claim, and the size before anything decodes it.
#[must_use]
pub fn is_displayable_image(mime: &str, data: &[u8]) -> bool {
    if data.len() > MAX_IMAGE_BYTES {
        return false;
    }
    if mime.eq_ignore_ascii_case("image/svg+xml") {
        return data.len() <= MAX_SVG_BYTES;
    }
    imagesize::blob_size(data).is_ok_and(|size| {
        (size.width as u64).saturating_mul(size.height as u64) <= MAX_IMAGE_PIXELS
    })
}

/// What a webview shows safely on its own: an image in `<img>` runs nothing, text is only text.
#[must_use]
pub fn can_preview(mime: &str) -> bool {
    const SHOWN: &[&str] =
        &["image/png", "image/jpeg", "image/gif", "image/webp", "image/bmp", "image/svg+xml", "text/plain"];
    SHOWN.iter().any(|shown| shown.eq_ignore_ascii_case(mime))
}

pub const FALLBACK: &str = "download";
/// Most file systems stop at 255 bytes.
pub const MAX_NAME_CHARS: usize = 200;

/// Some extensions are this long, and no real one is longer.
const MAX_EXTENSION_CHARS: usize = 16;
const RESERVED_ON_WINDOWS: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
    "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Invisible, or able to reorder what is shown: `invoice\u{202e}fdp.exe` looks like a PDF, and a
/// zero-width space hides the difference between two names.
const fn is_deceptive(c: char) -> bool {
    matches!(
        c,
        '\u{ad}' | '\u{61c}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2069}' | '\u{feff}'
    )
}

/// A long name loses its middle, never the extension that decides what it does when opened.
fn shortened(name: &str) -> String {
    if name.chars().count() <= MAX_NAME_CHARS {
        return name.to_string();
    }
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    if extension.is_empty() || extension.chars().count() > MAX_EXTENSION_CHARS {
        return name.chars().take(MAX_NAME_CHARS).collect();
    }
    let stem: String = name.chars().take(MAX_NAME_CHARS - extension.chars().count() - 1).collect();
    format!("{}.{extension}", stem.trim_end())
}

/// An attachment's name comes from the sender, who may try `../../.ssh/config` or `CON`.
#[must_use]
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control() && !is_deceptive(*c) && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    // Windows drops trailing dots and spaces itself, so `a.exe .` would still end up a program.
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = shortened(clean.trim_matches(|c: char| c == '.' || c.is_whitespace()));
    let stem = clean.split('.').next().unwrap_or_default().to_ascii_lowercase();
    if clean.is_empty() {
        FALLBACK.to_string()
    } else if RESERVED_ON_WINDOWS.contains(&stem.as_str()) {
        format!("_{clean}")
    } else {
        clean
    }
}


#[cfg(test)]
mod tests {
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
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1,
        8, 6, 0, 0, 0,
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
}
