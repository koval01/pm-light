//! `--check-picture=<file>`: once the window is up, a made-up mail is drawn as a picture the way a real one is, written
//! to that file, and put on a clipboard. CI starts the app so on every system, with no account signed in, and keeps
//! the picture: it is the one look a system without a person at it gets at its web engine drawing mail.
//!
//! On macOS the clipboard is one of its own, never the person's; elsewhere it is the system's, so the flag is for CI.
//! Beside the picture, `<file>.txt` says what came of it, in numbers and words of this file only.

use super::{Purpose, draw, for_a_picture, options, stage, system};
use crate::{App, modal, print_dialog};
use proton_core::print::sample_document;
use std::{cell::RefCell, io::Cursor, path::PathBuf};

const ARGUMENT: &str = "--check-picture=";
/// Far longer than a picture takes: past it the check reports the stage it stopped at.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

thread_local! {
    static WRITE_TO: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// The file the check writes to, when the app was started for one.
#[must_use]
pub fn asked_in(arguments: impl Iterator<Item = String>) -> Option<PathBuf> {
    arguments.filter_map(|argument| argument.strip_prefix(ARGUMENT).map(PathBuf::from)).find(|path| !path.as_os_str().is_empty())
}

/// From `main`, once the window is up.
pub fn run_if_asked(app: &App) {
    let Some(path) = asked_in(std::env::args().skip(1)) else { return };
    log::info!("a check of the mail picture asked for");
    WRITE_TO.set(Some(path));
    modal::after_show(app, |app| {
        let prepared = sample_document(&options(true), &print_dialog::labels(&app));
        draw(&app, for_a_picture(prepared), Purpose::Check);
        slint::Timer::single_shot(PATIENCE, || {
            if let Some(path) = WRITE_TO.take() {
                log::warn!("the mail picture stopped at: {}", stage());
                let mut note = path.into_os_string();
                note.push(".txt");
                let _ = std::fs::write(PathBuf::from(note), format!("failed\nstopped at: {}\n", stage()));
            }
        });
    });
}

/// What `<file>.txt` holds.
#[must_use]
pub fn report(png: &[u8], cut: bool, copied: bool) -> String {
    let size = image::ImageReader::new(Cursor::new(png)).with_guessed_format().ok().and_then(|reader| reader.into_dimensions().ok());
    let (width, height) = size.unwrap_or((0, 0));
    format!("width={width}\nheight={height}\nbytes={}\ncut={cut}\ncopied={copied}\n", png.len())
}

/// The picture is drawn: to the file, and onto a clipboard.
pub fn finish(png: &[u8], cut: bool) {
    let Some(path) = WRITE_TO.take() else { return };
    let copied = system::copy_checked(png);
    let mut note = path.clone().into_os_string();
    note.push(".txt");
    let written = std::fs::write(&path, png).is_ok() && std::fs::write(PathBuf::from(note), report(png, cut, copied)).is_ok();
    log::info!("the mail picture checked: {} KB, copied {copied}, written {written}", png.len() / 1024);
}

/// The check could not draw: the note says so, and no picture is left to be taken for one.
pub fn failed() {
    let Some(path) = WRITE_TO.take() else { return };
    let mut note = path.into_os_string();
    note.push(".txt");
    let _ = std::fs::write(PathBuf::from(note), "failed\n");
}

#[cfg(test)]
mod tests;
