//! `--check-window`: the window goes through the ways a person keeps it, on a fixed timetable, for CI to photograph:
//! as it opened, maximised, full screen, and back. CI starts the app so and takes a picture of the screen at the
//! middle of each step (`.github/workflows/build.yml`).

use crate::App;
use slint::{ComponentHandle, Timer};
use std::time::Duration;

const ARGUMENT: &str = "--check-window";

/// Each step, from the start of the check: CI photographs at the middle of each.
pub const STEPS: [(Duration, &str); 3] = [
    (Duration::from_secs(10), "maximised"),
    (Duration::from_secs(15), "full screen"),
    (Duration::from_secs(20), "as it opened"),
];

#[must_use]
pub fn asked_in(mut arguments: impl Iterator<Item = String>) -> bool {
    arguments.any(|argument| argument == ARGUMENT)
}

/// From `main`, once the window is up.
pub fn run_if_asked(app: &App) {
    if !asked_in(std::env::args().skip(1)) {
        return;
    }
    log::info!("a check of the window's ways asked for");
    for (at, step) in STEPS {
        let weak = app.as_weak();
        Timer::single_shot(at, move || {
            let Some(app) = weak.upgrade() else { return };
            let window = app.window();
            match step {
                "maximised" => window.set_maximized(true),
                "full screen" => window.set_fullscreen(true),
                _ => {
                    window.set_fullscreen(false);
                    window.set_maximized(false);
                }
            }
            log::info!("the window: {step}");
        });
    }
}

#[cfg(test)]
mod tests;
