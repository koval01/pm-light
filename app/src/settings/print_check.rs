//! Checking a printed mail against what signed it. A code
//! knows nothing of the paper it is on, and the words say so.

use super::{
    Ctx,
    rows::{SettingsRow, area, button, error, ghost, note, plain, strong},
};
use crate::App;
use proton_core::{
    PrintCheck, SealCheck,
    error::Error,
    seal::{self, SealedMail},
    service::SheetReport,
};
use rfd::FileDialog;
use slint::ComponentHandle;

const CODES: &str = "print-codes";
const SCAN: &str = "print-scan";
const CHECK: &str = "print-check";
const RESULT: &str = "print-result";
const SCAN_TYPES: [&str; 5] = ["png", "jpg", "jpeg", "webp", "pdf"];
const LARGEST_SCAN: u64 = 32 * 1024 * 1024;
const PASTE_LINES: i32 = 5;

pub fn page(ctx: &Ctx) {
    ctx.show(vec![
        plain(vec![
            note(&ctx.say("printCheck.about")),
            button(SCAN, &ctx.say("printCheck.scan"), true),
            area(CODES, &ctx.say("printCheck.paste"), "", PASTE_LINES),
            ghost(CHECK, &ctx.say("printCheck.check"), false),
        ]),
        super::rows::section(RESULT, "", "", Vec::new()),
        plain(vec![note(&ctx.say("printCheck.phone"))]),
    ]);
}

fn original(ctx: &Ctx, mail: &SealedMail) -> Vec<SettingsRow> {
    vec![
        strong(&ctx.say_with("printCheck.genuine", &[("signer", &mail.signer)])),
        note(&ctx.say("printCheck.compare")),
        note(&format!("{} · {}", mail.sender, super::rows::moment(mail.date))),
        note(&mail.subject),
        note(&mail.text),
    ]
}

fn seal_rows(ctx: &Ctx, check: &SealCheck) -> Vec<SettingsRow> {
    match check {
        SealCheck::Nothing => Vec::new(),
        SealCheck::Incomplete { signer, have, need } => vec![
            strong(&ctx.say_with("printCheck.genuine", &[("signer", signer)])),
            note(&ctx.say_with("printCheck.incomplete", &[("have", &have.to_string()), ("need", &need.to_string())])),
        ],
        SealCheck::NoSignature { .. } => vec![strong(&ctx.say("printCheck.noSignature"))],
        SealCheck::Damaged => vec![strong(&ctx.say("printCheck.damaged"))],
        SealCheck::NoKey { signer } => vec![strong(&ctx.say_with("printCheck.noKey", &[("signer", signer)]))],
        SealCheck::Forged { signer } => vec![error(&ctx.say_with("printCheck.forged", &[("signer", signer)]))],
        SealCheck::Genuine { mail } => original(ctx, mail),
    }
}

fn code_row(ctx: &Ctx, check: &PrintCheck) -> Option<SettingsRow> {
    let key = match check {
        PrintCheck::Unknown => "printCheck.unknown",
        PrintCheck::Matches { .. } => "printCheck.matches",
        PrintCheck::Differs { .. } => "printCheck.differs",
        PrintCheck::Unreadable => return None,
    };
    Some(note(&ctx.say(key)))
}

fn show_report(ctx: &Ctx, report: &SheetReport) {
    let mut lines = seal_rows(ctx, &report.seal);
    lines.extend(report.print.as_ref().and_then(|check| code_row(ctx, check)));
    if lines.is_empty() {
        lines.push(note(&ctx.say("printCheck.nothing")));
    }
    ctx.replace(RESULT, lines);
}

fn read_scans(parent: &App) -> Result<Option<Vec<Vec<u8>>>, Error> {
    let dialog = FileDialog::new().set_parent(&parent.window().window_handle()).add_filter("Scans", &SCAN_TYPES);
    let Some(picked) = dialog.pick_files() else { return Ok(None) };
    let mut files = Vec::new();
    for path in picked {
        if std::fs::metadata(&path).map_or(u64::MAX, |meta| meta.len()) > LARGEST_SCAN {
            return Err(Error::UnreadableScan);
        }
        files.push(std::fs::read(&path).map_err(|_| Error::UnreadableScan)?);
    }
    Ok(Some(files))
}

fn scan(ctx: &Ctx) {
    let Some(app) = ctx.app.upgrade() else { return };
    match read_scans(&app) {
        Ok(Some(files)) => {
            let service = ctx.service.clone();
            ctx.tell(&ctx.say("printCheck.reading"));
            ctx.load(async move { service.check_scans(files).await }, |ctx, report| show_report(ctx, &report));
        }
        Ok(None) => {}
        Err(failure) => ctx.report(&failure),
    }
}

pub fn heard(ctx: &Ctx, id: &str) {
    match id {
        SCAN => scan(ctx),
        CHECK => {
            let (service, codes) = (ctx.service.clone(), seal::pasted_codes(&ctx.text(CODES)));
            ctx.load(async move { service.check_sheets(codes, Vec::new()).await }, |ctx, report| {
                show_report(ctx, &report);
            });
        }
        _ => {}
    }
}

pub fn typed(ctx: &Ctx) {
    ctx.enable(CHECK, !ctx.text(CODES).trim().is_empty());
}
