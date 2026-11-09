//! The tour of the window for someone who opened the app for the first time. It waits for the mailbox, so that what
//! it points at is there, and it is shown once: leaving it at any step counts as having seen it.

use crate::{App, preferences};
use slint::{ComponentHandle, Timer};
use std::time::Duration;

/// Long enough for the first folder to be drawn under it.
const AFTER_THE_MAILBOX: Duration = Duration::from_millis(900);

/// After the mailbox opened: the tour, if a first start left it waiting.
pub fn offer(app: &App) {
    if !preferences::get().tour_pending || app.get_tour_step() > 0 {
        return;
    }
    // A check run by a machine looks at the mail, which the tour would cover; it stays waiting for a person.
    if crate::account_check::asked() {
        log::info!("the tour left for a person, in an account check");
        return;
    }
    let weak = app.as_weak();
    Timer::single_shot(AFTER_THE_MAILBOX, move || {
        if let Some(app) = weak.upgrade() {
            start(&app);
        }
    });
}

/// From the first step, as the button in the settings asks.
pub fn start(app: &App) {
    log::info!("the tour began");
    // The mail's page sits above everything the toolkit draws: out of sight while the tour is on.
    if app.get_tour_step() == 0 {
        crate::mail_view::covered(true);
    }
    app.set_tour_step(1);
}

pub fn connect(app: &App) {
    let weak = app.as_weak();
    app.on_tour_ended(move |skipped, step| {
        crate::mail_view::covered(false);
        preferences::change(|preferences| preferences.tour_pending = false);
        log::info!("the tour {} at step {step}", if skipped { "was left" } else { "ended" });
        if let Some(app) = weak.upgrade() {
            crate::list_style::offer(&app);
        }
    });
}
