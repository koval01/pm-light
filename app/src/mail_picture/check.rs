//! `--check-picture=<file>`: once the window is up, a made-up mail is drawn as a picture the way a real one is, written
//! to that file, and put on a clipboard. CI starts the app so on every system, with no account signed in, and keeps
//! the picture: it is the one look a system without a person at it gets at its web engine drawing mail.
//!
//! On macOS the clipboard is one of its own, never the person's; elsewhere it is the system's, so the flag is for CI.
//! Beside the picture, `<file>.txt` says what came of it, in numbers and words of this file only.
//!
//! Then the same mail is drawn once more, and once its page has loaded, the engine's page processes are ended, as a
//! person might in Task Manager: the picture must come back by itself. The note says so with `revived=true`.

use super::{Purpose, draw, for_a_picture, options, stage, system};
use crate::{App, modal, print_dialog};
use proton_core::print::sample_document;
use std::{
    cell::{Cell, RefCell},
    io::{Cursor, Write as _},
    path::PathBuf,
};

mod ending;

const ARGUMENT: &str = "--check-picture=";
/// Far longer than a picture takes: past it the check reports the stage it stopped at.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

thread_local! {
    static WRITE_TO: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static ROUND: Cell<Round> = const { Cell::new(Round::First) };
}

/// Which drawing of the check this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Round {
    First,
    /// Drawn again: the engine's processes are to be ended once the page has loaded.
    EndAtLoad,
    /// They were, this many: the picture has to come back by itself.
    Ended(usize),
}

/// The file the check writes to, when the app was started for one.
#[must_use]
pub fn asked_in(arguments: impl Iterator<Item = String>) -> Option<PathBuf> {
    arguments
        .filter_map(|argument| argument.strip_prefix(ARGUMENT).map(PathBuf::from))
        .find(|path| !path.as_os_str().is_empty())
}

/// From `main`, once the window is up.
pub fn run_if_asked(app: &App) {
    let Some(path) = asked_in(crate::program::arguments()) else { return };
    log::info!("a check of the mail picture asked for");
    WRITE_TO.set(Some(path));
    ROUND.set(Round::First);
    modal::after_show(app, |app| {
        draw_sample(&app);
        slint::Timer::single_shot(PATIENCE, || {
            if ROUND.get() == Round::First
                && let Some(path) = WRITE_TO.take()
            {
                log::warn!("the mail picture stopped at: {}", stage());
                let _ = std::fs::write(note_of(path), format!("failed\nstopped at: {}\n", stage()));
            }
        });
    });
}

fn draw_sample(app: &App) {
    let prepared = sample_document(&options(true), &print_dialog::labels(app));
    draw(app, for_a_picture(prepared), Purpose::Check);
}

fn note_of(path: PathBuf) -> PathBuf {
    let mut note = path.into_os_string();
    note.push(".txt");
    PathBuf::from(note)
}

/// Adds lines to the note, after what the first picture wrote.
fn add_to_note(path: PathBuf, lines: &str) {
    let note = std::fs::OpenOptions::new().append(true).open(note_of(path));
    let written = note.and_then(|mut note| note.write_all(lines.as_bytes()));
    if let Err(error) = written {
        log::warn!("the note of the picture check was not added to: {:?}", error.kind());
    }
}

/// The second round: the same mail, with the engine's processes ended on the way.
fn draw_again() {
    let Some(app) = crate::actions::main_window() else { return };
    log::info!("the mail picture is drawn again, and its engine ended on the way");
    ROUND.set(Round::EndAtLoad);
    draw_sample(&app);
    slint::Timer::single_shot(PATIENCE, || {
        if ROUND.get() != Round::First
            && let Some(path) = WRITE_TO.take()
        {
            log::warn!("the mail picture did not come back, stopped at: {}", stage());
            add_to_note(path, &format!("revived=false\nrevival stopped at: {}\n", stage()));
        }
    });
}

/// The check's page has loaded. In the second round, this is when its engine is ended.
pub fn page_loaded() {
    if ROUND.get() != Round::EndAtLoad {
        return;
    }
    let ended = ending::end_page_processes();
    log::info!("the picture check ended {ended} of the engine's page processes");
    ROUND.set(Round::Ended(ended));
    if ended == 0
        && let Some(path) = WRITE_TO.take()
    {
        add_to_note(path, "revived=false\nno page process of the engine was found to end\n");
    }
}

/// What `<file>.txt` holds.
#[must_use]
pub fn report(png: &[u8], cut: bool, copied: bool) -> String {
    let size = image::ImageReader::new(Cursor::new(png))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());
    let (width, height) = size.unwrap_or((0, 0));
    format!("width={width}\nheight={height}\nbytes={}\ncut={cut}\ncopied={copied}\n", png.len())
}

/// The picture is drawn: to the file, and onto a clipboard. In the second round, the note says it came back.
pub fn finish(png: &[u8], cut: bool) {
    if let Round::Ended(ended) = ROUND.get() {
        let Some(path) = WRITE_TO.take() else { return };
        log::info!("the mail picture came back after its engine was ended: {} KB", png.len() / 1024);
        add_to_note(path, &revived(png, ended));
        return;
    }
    let Some(path) = WRITE_TO.with_borrow(Clone::clone) else { return };
    let copied = system::copy_checked(png);
    let written = std::fs::write(&path, png).is_ok() && std::fs::write(note_of(path), report(png, cut, copied)).is_ok();
    log::info!("the mail picture checked: {} KB, copied {copied}, written {written}", png.len() / 1024);
    // On a later turn: the first picture's webview is still going.
    slint::Timer::single_shot(std::time::Duration::ZERO, draw_again);
}

/// What the note adds once the picture came back.
#[must_use]
pub fn revived(png: &[u8], ended: usize) -> String {
    let width = image::ImageReader::new(Cursor::new(png))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok())
        .map_or(0, |(width, _)| width);
    format!("revived=true\nrevived_width={width}\nprocesses_ended={ended}\n")
}

/// The check could not draw: the note says so, and no picture is left to be taken for one.
pub fn failed() {
    let Some(path) = WRITE_TO.take() else { return };
    if ROUND.get() == Round::First {
        let _ = std::fs::write(note_of(path), "failed\n");
    } else {
        add_to_note(path, &format!("revived=false\nrevival failed at: {}\n", stage()));
    }
}

#[cfg(test)]
mod tests;
