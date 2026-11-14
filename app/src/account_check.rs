//! `--check-account=<file>`: the app opens the account it has a session for, reads its folders and inbox, and draws
//! the first mail through the system's web engine, as the reader would. CI runs it on every system with the test
//! account's session, by hand and after the owner approves each run.
//!
//! It never refreshes the tokens, so the session it borrows stays the owner's; it never asks about Tor, and goes
//! straight to Proton. The first mail opens in the window, as a click would open it, and its drawing is kept as
//! `<file>.png`; then `screen_tour` walks the window through its pages for CI to photograph. The test account holds
//! nothing of worth, and its owner wants the pictures seen. The journal and `<file>.txt` carry numbers and fixed words
//! only.

use crate::{App, mail_picture, print_dialog};
use proton_core::{PrintScope, Service, model::ListOptions};
use slint::{ComponentHandle, Weak};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

const ARGUMENT: &str = "--check-account=";
/// Far longer than a start, a folder and a mail take: past it the note says where the check stopped.
const PATIENCE: Duration = Duration::from_secs(120);
/// Rows read to count the inbox by: enough to tell an empty inbox from a full one.
const ROWS: u64 = 50;

static ASKED: OnceLock<Option<PathBuf>> = OnceLock::new();

thread_local! {
    static WRITE_TO: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static STAGE: Cell<&'static str> = const { Cell::new("waiting for the session") };
    static FOUND: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// The file the check writes to, when the app was started for one.
#[must_use]
pub fn asked_in(arguments: impl Iterator<Item = String>) -> Option<PathBuf> {
    arguments
        .filter_map(|argument| argument.strip_prefix(ARGUMENT).map(PathBuf::from))
        .find(|path| !path.as_os_str().is_empty())
}

/// Whether this start is the account check.
pub fn asked() -> bool {
    ASKED.get_or_init(|| asked_in(std::env::args().skip(1))).is_some()
}

fn reached(stage: &'static str) {
    STAGE.set(stage);
    log::debug!("the account check: {stage}");
}

fn note(path: PathBuf, text: &str) {
    let mut name = path.into_os_string();
    name.push(".txt");
    let _ = std::fs::write(PathBuf::from(name), text);
}

/// What the note holds once the mail is drawn.
#[must_use]
pub fn report(folders: usize, rows: usize, width: u32, height: u32) -> String {
    format!("folders={folders}\ninbox rows={rows}\nwidth={width}\nheight={height}\n")
}

/// From `main`, once the window exists: the note is waited for from here.
pub fn begin() {
    let Some(path) = ASKED.get_or_init(|| asked_in(std::env::args().skip(1))).clone() else { return };
    log::info!("an account check asked for");
    WRITE_TO.set(Some(path));
    slint::Timer::single_shot(PATIENCE, || {
        if let Some(path) = WRITE_TO.take() {
            let stage = if STAGE.get() == "drawing" { mail_picture::stage() } else { STAGE.get() };
            log::warn!("the account check stopped at: {stage}");
            note(path, &format!("failed\nstopped at: {stage}\n"));
        }
    });
}

/// The mailbox is open: the inbox is read, and its first mail drawn.
pub fn mailbox_opened(service: &Arc<Service>, app: &Weak<App>, folders: usize) {
    if !asked() {
        return;
    }
    let _ = app.upgrade_in_event_loop(|_| reached("reading the inbox"));
    let (service, app) = (service.clone(), app.clone());
    tokio::spawn(async move {
        let options = ListOptions { limit: Some(ROWS), ..Default::default() };
        let rows = service.list("inbox", &options).await.unwrap_or_default();
        // An empty inbox still leaves a mail to draw when the account holds one anywhere: the newest of all mail, or a
        // draft, which the test account keeps for this.
        // A row is the open folder's: in conversations its id is a conversation's, which only that folder turns into
        // a message's. So a mail found in another folder is opened there.
        let mut first = rows.first().map(|row| row.id.clone());
        for folder in ["all", "drafts"] {
            if first.is_none() {
                first = service.list(folder, &options).await.unwrap_or_default().first().map(|row| row.id.clone());
                if first.is_some() {
                    service.view_open(folder).await;
                }
            }
        }
        let count = rows.len();
        let _ = app.upgrade_in_event_loop(move |app| {
            FOUND.set((folders, count));
            crate::screen_tour::remember(&service, rows);
            match first {
                Some(id) => draw_first(&service, &app, id),
                None => finished_without_mail(),
            }
        });
    });
}

fn draw_first(service: &Arc<Service>, app: &App, id: String) {
    reached("opening the first mail");
    crate::view::open_mail(service, &app.as_weak(), id.clone());
    let (service, weak, labels) = (service.clone(), app.as_weak(), print_dialog::labels(app));
    tokio::spawn(async move {
        let _ = service.view_select(Some(id)).await;
        let prepared = service.print_document(PrintScope::Message, &mail_picture::options(false), &labels).await;
        let _ = weak.upgrade_in_event_loop(move |app| match prepared {
            Ok(prepared) if !prepared.parts.is_empty() => {
                reached("drawing");
                mail_picture::draw_for_check(&app, prepared);
            }
            Ok(_) => failed("no mail to draw"),
            Err(error) => {
                log::warn!("the account check found no mail to draw: {}", error.kind());
                failed("mail not read");
            }
        });
    });
}

fn finished_without_mail() {
    let Some(path) = WRITE_TO.take() else { return };
    let (folders, _) = FOUND.get();
    log::info!("the account check: {folders} folders, an empty inbox");
    note(path, &report(folders, 0, 0, 0));
}

/// The first mail is drawn: its picture beside the note.
pub fn finish(png: &[u8], width: u32, height: u32) {
    let Some(path) = WRITE_TO.take() else { return };
    let (folders, rows) = FOUND.get();
    log::info!("the account check: {folders} folders, {rows} inbox rows, a mail drawn {width} by {height}");
    let mut picture = path.clone().into_os_string();
    picture.push(".png");
    let _ = std::fs::write(PathBuf::from(picture), png);
    note(path.clone(), &report(folders, rows, width, height));
    if let Some(app) = crate::actions::main_window() {
        crate::screen_tour::start(&app, path);
    }
}

/// The check could not go on.
pub fn failed(what: &'static str) {
    let Some(path) = WRITE_TO.take() else { return };
    log::warn!("the account check failed: {what}");
    note(path, &format!("failed\nstopped at: {what}\n"));
}

#[cfg(test)]
mod tests;
