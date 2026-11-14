//! What goes on paper. Rust decides the content and the marks, and the webview lays the text out.
//!
//! Paper size, orientation and scale stay in the system's print dialog: the page cannot set them
//! on every engine, and no two printers agree anyway. Sizes are therefore never taken on trust:
//! see `MM_ON_PAPER`, and the shift a person can calibrate with the test sheet.

use crate::{
    model::{Address, Message},
    seal::Seal,
    text::escape,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use qrcode::{EcLevel, QrCode, render::svg};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::Write;
use url::form_urlencoded;

/// How far one CSS millimetre lands on paper. WebKit shrinks a page less than CSS asks when it
/// prints, measured on macOS through `NSPrintOperation`. WebView2 prints true to size.
#[cfg(not(windows))]
const MM_ON_PAPER: f64 = 1.07;
#[cfg(windows)]
const MM_ON_PAPER: f64 = 1.0;

const TOP_MARGIN_MM: f64 = 18.0;
const BOTTOM_MARGIN_MM: f64 = 16.0;
/// Most printers cannot reach closer to the edge than this.
const MARK_LEFT_MM: f64 = 6.0;
const FOLD_LENGTH_MM: f64 = 7.0;
const HOLE_LENGTH_MM: f64 = 10.0;
const HOLE_MM: f64 = 148.5;
const MAX_SHIFT_MM: f64 = 20.0;
const RULER_STEP_MM: u32 = 10;
const RULER_END_MM: u32 = 270;
/// Printed size of the check code. It floats beside the heading, so the mail itself does not move.
const CODE_MM: f64 = 24.0;
/// The signed code, large enough for a phone to read from a desk.
const SIGNED_CODE_MM: f64 = 110.0;
/// A code holding more of the text, which a 300 dpi scan reads well.
const TEXT_CODE_MM: f64 = 72.0;
const CODES_ON_A_SHEET: usize = 6;
/// Where anyone can get a Proton address's public key, with no account.
const KEY_ADDRESS: &str = "https://api.protonmail.ch/pks/lookup?op=get&search=";
const CODE_PREFIX: &str = "PML1";
const FINGERPRINT_CONTEXT: &[u8] = b"proton-mail-light print 1\0";
/// Longer than any Proton id, so a pasted page of text is refused before anything is asked.
const LONGEST_ID: usize = 200;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FoldMarks {
    #[default]
    None,
    /// DIN 5008 form A: 87 and 192 mm.
    A,
    /// DIN 5008 form B: 105 and 210 mm.
    B,
}

impl FoldMarks {
    const fn positions(self) -> &'static [f64] {
        match self {
            Self::None => &[],
            Self::A => &[87.0, 192.0],
            Self::B => &[105.0, 210.0],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[expect(clippy::struct_excessive_bools, reason = "each is one box of the print dialog")]
pub struct PrintOptions {
    pub headers: bool,
    pub technical: bool,
    pub history: bool,
    pub images: bool,
    pub grayscale: bool,
    pub save_ink: bool,
    pub new_sheet_each: bool,
    pub fold_marks: FoldMarks,
    pub hole_mark: bool,
    /// Moves the marks down, for a printer that does not feed true.
    pub shift_mm: f64,
    /// A QR code that ties the sheet to the message it came from.
    pub check_code: bool,
    /// A last sheet of signed codes holding the message itself.
    pub seal: bool,
}

impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            headers: true,
            technical: false,
            history: false,
            images: true,
            grayscale: false,
            save_ink: false,
            new_sheet_each: true,
            fold_marks: FoldMarks::None,
            hole_mark: false,
            shift_mm: 0.0,
            check_code: false,
            seal: false,
        }
    }
}

/// In the person's language. The format is `strftime`, since dates here have no locale of their own.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrintLabels {
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: String,
    pub date_format: String,
    pub attachments: String,
    pub encryption: String,
    pub size: String,
    pub message_id: String,
    pub first_mail: String,
    pub earlier_mail: String,
    pub test_title: String,
    pub test_text: String,
    pub seal_title: String,
    pub seal_text: String,
    pub seal_key: String,
    pub seal_fingerprint: String,
    pub seal_more: String,
}

/// What to print about one message.
pub struct PrintItem<'a> {
    pub message: &'a Message,
    pub body: String,
    /// How many messages this sender wrote before this one, if asked for and known.
    pub earlier: Option<u64>,
    /// Its seal, if one was asked for.
    pub seal: Option<SealSheet>,
}

/// What a person needs to check a seal without this app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealSheet {
    pub seal: Seal,
    pub signer: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrintPart {
    /// Sits outside the mail's shadow root: the marks must not be reached by a mail's styles.
    pub marks: String,
    pub html: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrintDocument {
    pub style: String,
    /// Goes inside each shadow root, so that a wide mail fits the paper.
    pub fit: String,
    pub parts: Vec<PrintPart>,
}

fn address(a: &Address) -> String {
    a.name.as_ref().map_or_else(|| a.email.clone(), |name| format!("{name} <{}>", a.email))
}

fn addresses(list: &[Address]) -> String {
    list.iter().map(address).collect::<Vec<_>>().join(", ")
}

fn date_text(milliseconds: u64, format: &str) -> String {
    crate::timing::clock_text(i64::try_from(milliseconds / 1000).unwrap_or(0), format)
}

/// What a printed message says, in a form that nothing can blur: every part is counted by length,
/// so moving text from one field to the next changes the result.
#[must_use]
pub fn fingerprint(m: &Message) -> String {
    let mut hash = Sha256::new();
    hash.update(FINGERPRINT_CONTEXT);
    let mut part = |bytes: &[u8]| {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    };
    part(m.meta.from.email.as_bytes());
    for person in m.meta.to.iter().chain(&m.cc) {
        part(person.email.as_bytes());
    }
    part(&m.meta.date.to_le_bytes());
    part(m.meta.subject.as_bytes());
    part(m.body.as_bytes());
    for file in &m.attachments {
        part(file.name.as_bytes());
        part(&file.size.to_le_bytes());
    }
    URL_SAFE_NO_PAD.encode(hash.finalize())
}

#[must_use]
pub fn check_code(m: &Message) -> String {
    format!("{CODE_PREFIX}.{}.{}", m.meta.id, fingerprint(m))
}

/// What a scanned code holds, or nothing if it is not one of ours. It may be anything: the id goes
/// into a request, so only the characters of a Proton id get through.
#[must_use]
pub fn parse_check_code(code: &str) -> Option<(&str, &str)> {
    let mut parts = code.trim().split('.');
    let (prefix, id, fingerprint) = (parts.next()?, parts.next()?, parts.next()?);
    let id_is_clean = |id: &str| {
        !id.is_empty()
            && id.len() <= LONGEST_ID
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'=' | b'+' | b'/'))
    };
    (prefix == CODE_PREFIX && parts.next().is_none() && id_is_clean(id) && !fingerprint.is_empty())
        .then_some((id, fingerprint))
}

fn code_image(text: &str, size_mm: f64, level: EcLevel) -> String {
    let Ok(code) = QrCode::with_error_correction_level(text, level) else { return String::new() };
    let svg = code.render::<svg::Color<'_>>().quiet_zone(true).build();
    let size = size_mm / MM_ON_PAPER;
    let svg = svg.replacen("<svg ", &format!(r#"<svg style="width:{size:.2}mm;height:{size:.2}mm" "#), 1);
    svg.replacen(r#"<?xml version="1.0" standalone="yes"?>"#, "", 1)
}

fn check_code_image(m: &Message) -> String {
    format!(r#"<div class="check-code">{}</div>"#, code_image(&check_code(m), CODE_MM, EcLevel::M))
}

fn seal_sheet(sheet: Option<&SealSheet>, labels: &PrintLabels) -> String {
    let Some(sheet) = sheet else { return String::new() };
    let total = sheet.seal.text_codes.len();
    let cell = |code: &str, caption: String| {
        let image = code_image(code, TEXT_CODE_MM, EcLevel::Q);
        format!(r#"<figure class="seal-cell">{image}<figcaption>{caption}</figcaption></figure>"#)
    };
    let cells: Vec<String> = sheet
        .seal
        .text_codes
        .iter()
        .enumerate()
        .map(|(index, code)| cell(code, format!("{}/{total}", index + 1)))
        .collect();
    // Whole sheets of them: WebKit cuts a code in two where a grid runs off the end of a page.
    let more = cells.chunks(CODES_ON_A_SHEET).fold(String::new(), |mut more, sheet_of_codes| {
        // Writing to a String cannot fail.
        let _ = write!(more, r#"<div class="seal-codes seal-page">{}</div>"#, sheet_of_codes.concat());
        more
    });
    // On the other side of the sheet: a streak down the paper cannot take both.
    let copy = format!(r#"<div class="seal-codes seal-copy">{}</div>"#, cell(&sheet.seal.statement, "PGP".to_string()));
    let note = if cells.is_empty() { String::new() } else { format!("<p>{}</p>", escape(&labels.seal_more)) };
    let more = format!("{note}{copy}{more}");
    let key: String = form_urlencoded::byte_serialize(sheet.signer.as_bytes()).collect();
    format!(
        r#"<div class="seal-sheet"><h2>{}</h2><p>{}</p>{}<p class="seal-key">{}: {KEY_ADDRESS}{key}<br>{}: {}</p>{more}</div>"#,
        escape(&labels.seal_title),
        escape(&labels.seal_text),
        code_image(&sheet.seal.statement, SIGNED_CODE_MM, EcLevel::Q),
        escape(&labels.seal_key),
        escape(&labels.seal_fingerprint),
        escape(&sheet.fingerprint),
    )
}

fn row(name: &str, value: &str) -> String {
    format!(r"<tr><th>{}</th><td>{}</td></tr>", escape(name), escape(value))
}

fn header_rows(item: &PrintItem<'_>, labels: &PrintLabels) -> String {
    let m = item.message;
    let mut rows = vec![row(&labels.from, &address(&m.meta.from)), row(&labels.to, &addresses(&m.meta.to))];
    if !m.cc.is_empty() {
        rows.push(row(&labels.cc, &addresses(&m.cc)));
    }
    rows.push(row(&labels.date, &date_text(m.meta.date, &labels.date_format)));
    if !m.attachments.is_empty() {
        let names: Vec<&str> = m.attachments.iter().map(|file| file.name.as_str()).collect();
        rows.push(row(&labels.attachments, &names.join(", ")));
    }
    rows.concat()
}

fn technical_rows(m: &Message, labels: &PrintLabels) -> String {
    let details = &m.details;
    let mut rows = vec![row(&labels.encryption, &details.encryption), row(&labels.size, &format!("{} B", m.size))];
    if !details.message_id.is_empty() {
        rows.push(row(&labels.message_id, &details.message_id));
    }
    for check in &details.auth {
        rows.push(row(&check.method.to_uppercase(), &check.result));
    }
    rows.concat()
}

fn history_row(item: &PrintItem<'_>, labels: &PrintLabels) -> String {
    let Some(earlier) = item.earlier else { return String::new() };
    let text = if earlier == 0 {
        labels.first_mail.clone()
    } else {
        labels.earlier_mail.replace("{count}", &earlier.to_string())
    };
    format!(r#"<p class="history">{}</p>"#, escape(&text))
}

fn heading(item: &PrintItem<'_>, options: &PrintOptions, labels: &PrintLabels) -> String {
    let m = item.message;
    let code = if options.check_code { check_code_image(m) } else { String::new() };
    let subject = format!("{code}<h1>{}</h1>", escape(&m.meta.subject));
    let mut rows = String::new();
    if options.headers {
        rows.push_str(&header_rows(item, labels));
    }
    if options.technical {
        rows.push_str(&technical_rows(m, labels));
    }
    let table = if rows.is_empty() { String::new() } else { format!("<table>{rows}</table>") };
    // A block of its own: its rules share the shadow root with the mail, and must not reach the mail's tables.
    format!(r#"<div class="print-head">{subject}{table}{}<hr></div>"#, history_row(item, labels))
}

/// Where a mark must be drawn for it to land at `sheet_mm` from the top of the paper. The page's
/// margin reaches the paper as it is, and only what is inside it is scaled.
fn mark_top(sheet_mm: f64, shift_mm: f64) -> f64 {
    (sheet_mm + shift_mm.clamp(-MAX_SHIFT_MM, MAX_SHIFT_MM) - TOP_MARGIN_MM) / MM_ON_PAPER
}

fn mark(sheet_mm: f64, shift_mm: f64, length_mm: f64) -> String {
    format!(
        r#"<i class="mark" style="top:{:.2}mm;width:{:.2}mm"></i>"#,
        mark_top(sheet_mm, shift_mm),
        length_mm / MM_ON_PAPER
    )
}

fn marks(options: &PrintOptions) -> String {
    let folds = options.fold_marks.positions().iter();
    let mut out: String = folds.map(|at| mark(*at, options.shift_mm, FOLD_LENGTH_MM)).collect();
    if options.hole_mark {
        out.push_str(&mark(HOLE_MM, options.shift_mm, HOLE_LENGTH_MM));
    }
    out
}

fn style(options: &PrintOptions) -> String {
    let break_rule = if options.new_sheet_each { "page" } else { "auto" };
    let adjust = if options.save_ink { "economy" } else { "exact" };
    format!(
        "@page{{margin:{TOP_MARGIN_MM}mm 0 {BOTTOM_MARGIN_MM}mm 0}}\
         #print-root{{color:#000;background:#fff;font:11pt/1.4 system-ui,sans-serif;\
         print-color-adjust:{adjust};-webkit-print-color-adjust:{adjust}}}\
         .print-part{{position:relative;padding:0 18mm 0 22mm}}\
         .print-part+.print-part{{break-before:{break_rule};margin-top:8mm}}\
         .mark{{position:absolute;left:{:.2}mm;border-top:.25mm solid #000}}",
        MARK_LEFT_MM / MM_ON_PAPER
    )
}

/// Mail is written for screens of any width. Paper has one. WebKit ignores `max-width` on a table, so a table of a fixed
/// width is laid out by its content instead; one that asks for a share of the page keeps it, and so do its cells'
/// padding and the mail's own headings, which the rules of the printout's heading do not reach.
fn fit(options: &PrintOptions) -> String {
    let ink = if options.save_ink {
        "*{background:none!important;color:#000!important;box-shadow:none!important}"
    } else {
        ""
    };
    // A filter over the page would turn its text into a blurred picture, so text stays type.
    let gray = if options.grayscale {
        ".print-body img{filter:grayscale(1) contrast(1.1)}\
         .print-body *{color:#000!important;background-color:transparent!important;border-color:#000!important}"
    } else {
        ""
    };
    format!(
        ".print-head h1{{font-size:16pt;margin:0 0 3mm;overflow-wrap:anywhere;break-after:avoid}}\
         .print-head table{{border-collapse:collapse;max-width:100%}}\
         .print-head th{{text-align:left;font-weight:600;padding:.4mm 4mm .4mm 0;vertical-align:top;white-space:nowrap}}\
         .print-head td{{padding:.4mm 0;overflow-wrap:anywhere}}\
         .print-head hr{{border:0;border-top:.25mm solid #888;margin:3mm 0 5mm;clear:both}}\
         .check-code{{float:right;margin:0 0 2mm 4mm}}\
         .seal-sheet{{break-before:page}}\
         .seal-sheet h2{{font-size:13pt;margin:0 0 2mm}}\
         .seal-sheet p{{font-size:9pt;margin:0 0 3mm}}\
         .seal-sheet>svg{{display:block;margin:0 0 3mm}}\
         .seal-key{{font-size:8pt;line-height:1.4;overflow-wrap:anywhere}}\
         .seal-codes{{display:grid;grid-template-columns:repeat(2,max-content);gap:4mm 6mm;justify-content:start}}\
         .seal-page{{break-before:page}}\
         .seal-copy{{grid-template-columns:max-content;justify-content:end}}\
         .seal-cell{{margin:0;break-inside:avoid}}\
         .seal-cell svg{{display:block}}\
         .seal-cell figcaption{{font-size:8pt;text-align:center}}\
         .seal-key{{overflow-wrap:anywhere}}\
         .history{{margin:2mm 0 0;font-weight:600}}\
         .print-body{{overflow-wrap:anywhere}}\
         .print-body *{{max-width:100%!important;box-sizing:border-box}}\
         .print-body img{{height:auto!important;break-inside:avoid}}\
         .print-body table:not([width$=\"%\"]):not([style*=\"%\"]){{width:auto!important;table-layout:auto}}\
         .print-body td,.print-body th{{overflow-wrap:anywhere}}\
         .print-body pre{{white-space:pre-wrap}}\
         .print-body blockquote,.print-body tr{{break-inside:avoid}}\
         {gray}{ink}"
    )
}

/// A code as pixels for the screen, with its quiet zone: black on white, four bytes each.
#[must_use]
pub fn qr_pixels(text: &str, module: u32) -> Option<(Vec<u8>, u32)> {
    const QUIET_MODULES: u32 = 4;
    let code = QrCode::with_error_correction_level(text, EcLevel::M).ok()?;
    let width = u32::try_from(code.width()).ok()?;
    let side = (width + QUIET_MODULES * 2) * module;
    let colors = code.to_colors();
    let mut pixels = vec![255_u8; (side * side * 4) as usize];
    for (at, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let at = u32::try_from(at).ok()?;
        let (x, y) = ((at % side) / module, (at / side) / module);
        let inside = x >= QUIET_MODULES && y >= QUIET_MODULES && x < width + QUIET_MODULES && y < width + QUIET_MODULES;
        if inside {
            let place = ((y - QUIET_MODULES) * width + (x - QUIET_MODULES)) as usize;
            if colors.get(place).is_some_and(|color| *color == qrcode::Color::Dark) {
                pixel[..3].fill(0);
            }
        }
    }
    Some((pixels, side))
}

#[must_use]
pub fn document(items: &[PrintItem<'_>], options: &PrintOptions, labels: &PrintLabels) -> PrintDocument {
    let parts = items.iter().enumerate().map(|(index, item)| PrintPart {
        // A folded letter is held by its first sheet, and engines differ on repeating anything.
        marks: if index == 0 || options.new_sheet_each { marks(options) } else { String::new() },
        html: format!(
            r#"{}<div class="print-body">{}</div>{}"#,
            heading(item, options, labels),
            item.body,
            seal_sheet(item.seal.as_ref(), labels)
        ),
    });
    PrintDocument { style: style(options), fit: fit(options), parts: parts.collect() }
}

/// One sheet with a scale along its edge: laid against a real ruler, it shows what shift to set.
#[must_use]
pub fn test_sheet(options: &PrintOptions, labels: &PrintLabels) -> PrintDocument {
    let mut ticks =
        (2..=RULER_END_MM / RULER_STEP_MM).map(|step| step * RULER_STEP_MM).fold(String::new(), |mut ticks, at| {
            let top = mark_top(f64::from(at), options.shift_mm);
            let _ = write!(
                ticks,
                r#"<i class="mark" style="top:{top:.2}mm;width:4mm"></i><b class="tick" style="top:{:.2}mm">{at}</b>"#,
                top - 1.6
            );
            ticks
        });
    ticks.push_str(&marks(options));
    let html = format!("<h1>{}</h1><p>{}</p>", escape(&labels.test_title), escape(&labels.test_text));
    let style = format!("{}.tick{{position:absolute;left:9mm;font:7pt system-ui,sans-serif}}", style(options));
    PrintDocument { style, fit: fit(options), parts: vec![PrintPart { marks: ticks, html }] }
}

mod sample;
pub use sample::sample_document;

#[cfg(test)]
mod tests;
