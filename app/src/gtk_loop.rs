//! On Linux the menus, the tray icon and every webview are GTK's, while the window is winit's, as Slint draws it. GTK
//! is started once on the main thread before any of them, and its events are handed on from the window's own loop:
//! without this the first menu panics ("GTK has not been initialized") and no webview draws.

use slint::{Timer, TimerMode};
use std::time::Duration;

/// About a frame: often enough for a webview to draw and take input smoothly.
const EVERY: Duration = Duration::from_millis(16);

thread_local! {
    static PUMP: Timer = Timer::default();
}

/// Before anything of GTK's is made. False when there is no display GTK can open: the app then runs without what
/// needs GTK, as far as it can.
pub fn start() -> bool {
    let started = gtk::init().is_ok();
    if !started {
        log::warn!("GTK did not start");
    }
    started
}

/// Once the event loop runs: GTK's pending events, on every turn.
pub fn pump() {
    PUMP.with(|timer| {
        timer.start(TimerMode::Repeated, EVERY, || {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        });
    });
}
