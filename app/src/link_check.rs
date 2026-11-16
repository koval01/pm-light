//! `--soak-link-port=<port>`: before the long run's own actions, a made-up mail that is one long link is shown in the
//! real mail view, and CI presses it with a real pointer (`.github/workflows/long-run.yml`). The link points at a
//! port on CI's machine that counts the connections it gets, and goes through `html::protect` as every mail's link
//! does, so it asks for a window of its own. The press must reach the way to the browser (refused in a check, which
//! the journal names), and nothing may reach the port: WebKit connected to a clicked link's address on its own before
//! the app said no, and that went around Tor (`nowhere.rs`). The app writes where the mail's body is on its window to
//! `<note>.link`, and goes on once CI writes `<note>.link-done`, or after two minutes.

use crate::App;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{fmt::Write as _, path::PathBuf, time::Duration};

const PORT: &str = "--soak-link-port=";
/// The made-up mail is drawn and fitted by then.
const DRAWN: Duration = Duration::from_secs(3);
const LOOK: Duration = Duration::from_millis(500);
const LONGEST: Duration = Duration::from_secs(120);

/// The port asked for, when this long run checks a link first.
pub fn port_in(mut arguments: impl Iterator<Item = String>) -> Option<u16> {
    arguments.find_map(|argument| argument.strip_prefix(PORT)?.parse().ok()).filter(|port| *port != 0)
}

/// A mail that is a link and nothing else, long enough to fill the body, cleaned as a mail from anyone would be.
pub fn page(port: u16) -> String {
    let mut words = String::new();
    for _ in 0..400 {
        let _ = write!(words, "a link to a port that counts ");
    }
    let raw = format!("<p><a href=\"http://127.0.0.1:{port}/link-check\">{words}</a></p>");
    proton_core::html::protect(&raw).html
}

/// The mail's body on the window, in the window's own pixels: left, top, width and height.
fn body(app: &App) -> [u32; 4] {
    let scale = app.window().scale_factor() * crate::numbers::scale(crate::look::zoom());
    let pixels = |length: f32| crate::numbers::pixels(length * scale);
    [pixels(app.get_body_x()), pixels(app.get_body_y()), pixels(app.get_body_width()), pixels(app.get_body_height())]
}

/// Room enough to press a link in the middle of.
const ROOM: u32 = 40;

thread_local! {
    static WAITING: Timer = Timer::default();
}

/// Shows the link, says where it is, and runs `then` once CI has pressed it, or when it waited long enough.
pub fn run(app: &App, note: PathBuf, then: impl FnOnce(&App) + 'static) {
    let Some(port) = port_in(std::env::args().skip(1)) else {
        then(app);
        return;
    };
    log::info!("the link check: a mail that is one link, shown");
    crate::mail_view::show_finished(app, &page(port));
    let (weak, done) = (app.as_weak(), with_end(&note, ".link-done"));
    let then = std::cell::Cell::new(Some(then));
    Timer::single_shot(DRAWN, move || {
        let Some(app) = weak.upgrade() else { return };
        if !measure(&app) {
            log::warn!("the link check: the page is not laid out at its place");
        }
        ready_for_press(&app, &note, done, then.take());
    });
}

/// Says where to press, and runs `then` once CI has pressed it, or when it waited long enough.
fn ready_for_press(app: &App, note: &std::path::Path, done: PathBuf, then: Option<impl FnOnce(&App) + 'static>) {
    let [left, top, width, height] = body(app);
    let shown = crate::mail_view::on_screen();
    log::info!("the link check: the body is {width} by {height} at {left}, {top}; the page on screen: {shown}");
    if shown && width >= ROOM && height >= ROOM {
        let (x, y) = (left + width / 2, top + height / 2);
        let _ = std::fs::write(with_end(note, ".link"), format!("{x} {y}\n"));
        log::info!("the link check: waiting for the press at {x} by {y}");
    } else {
        let _ = std::fs::write(with_end(note, ".link"), "nowhere\n");
        log::warn!("the link check: nothing to press");
    }
    let (started, weak, then) = (std::time::Instant::now(), app.as_weak(), std::cell::Cell::new(then));
    WAITING.with(|waiting| {
        waiting.start(TimerMode::Repeated, LOOK, move || {
            let pressed = done.exists();
            if !pressed && started.elapsed() < LONGEST {
                return;
            }
            log::info!("the link check: {}", if pressed { "pressed" } else { "no press came" });
            WAITING.with(Timer::stop);
            if let (Some(app), Some(then)) = (weak.upgrade(), then.take()) {
                then(&app);
            }
        });
    });
}

/// The place of the page, in logical pixels.
fn place(app: &App) -> (i32, i32) {
    let zoom = crate::numbers::scale(crate::look::zoom());
    let logical = |length: f32| i32::try_from(crate::numbers::pixels(length * zoom)).unwrap_or(i32::MAX);
    (logical(app.get_body_width()), logical(app.get_body_height()))
}

/// Says in the journal GTK's sizes of the page's window and of the page beside its place, and whether they agree.
fn measure(app: &App) -> bool {
    let place = place(app);
    let Some(Some([window, laid])) = crate::mail_view::with_page(crate::webviews::laid_out) else { return true };
    log::info!(
        "the link check: the window is {} by {}, the page laid out {} by {}, its place {} by {}",
        window.0,
        window.1,
        laid.0,
        laid.1,
        place.0,
        place.1
    );
    laid == place
}

fn with_end(note: &std::path::Path, end: &str) -> PathBuf {
    let mut path = note.as_os_str().to_owned();
    path.push(end);
    PathBuf::from(path)
}

#[cfg(test)]
mod tests;
