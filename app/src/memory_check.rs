//! After the screen tour, the account check opens every mail of the inbox again and again, and writes down how much
//! memory the app takes after each round, its webviews' processes included. The first round fills what is meant to be
//! filled, caches and the engine's pages; after it a steady app stays level, and one that keeps what it no longer shows
//! grows round by round. `<file>.memory` holds the figures, numbers only, for CI to judge
//! (`.github/workflows/live.yml`), and the journal says the same.

use crate::{App, memory, view};
use proton_core::{Service, model::MessageMeta};
use slint::{ComponentHandle, Timer, TimerMode, Weak};
use std::{cell::RefCell, fmt::Write as _, path::PathBuf, sync::Arc, time::Duration};

/// Rounds over the whole inbox: one to fill, the rest to measure.
pub const ROUNDS: usize = 6;
/// Between two mails: enough for one to be drawn before the next replaces it.
const BETWEEN: Duration = Duration::from_millis(1200);
/// After a round, before the figure is taken, for what was let go to be given back.
const RESTING: Duration = Duration::from_secs(4);

struct Check {
    service: Arc<Service>,
    app: Weak<App>,
    ids: Vec<String>,
    opened: usize,
    figures: Vec<u64>,
    file: PathBuf,
    then: fn(),
    timer: Timer,
}

thread_local! {
    static CHECK: RefCell<Option<Check>> = const { RefCell::new(None) };
}

/// Opens the inbox's mails `ROUNDS` times, then writes `file` and calls `then`.
pub fn run(service: &Arc<Service>, app: &App, rows: &[MessageMeta], file: PathBuf, then: fn()) {
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    if ids.is_empty() {
        then();
        return;
    }
    log::info!("the memory check: {ROUNDS} rounds of {} mails", ids.len());
    let check = Check { service: service.clone(), app: app.as_weak(), ids, opened: 0, figures: Vec::new(), file, then, timer: Timer::default() };
    CHECK.set(Some(check));
    after(BETWEEN, step);
}

fn after(delay: Duration, then: fn()) {
    CHECK.with_borrow(|check| {
        if let Some(check) = check {
            check.timer.start(TimerMode::SingleShot, delay, then);
        }
    });
}

/// The next mail, or the figure at the end of a round.
fn step() {
    let next = CHECK.with_borrow_mut(|check| {
        let check = check.as_mut()?;
        let count = check.ids.len();
        if check.opened > 0 && check.opened % count == 0 && check.figures.len() < check.opened / count {
            return Some(None);
        }
        let id = check.ids.get(check.opened % count)?.clone();
        check.opened += 1;
        Some(Some((check.service.clone(), check.app.clone(), id)))
    });
    match next {
        Some(Some((service, app, id))) => {
            view::open_mail(&service, &app, id);
            after(BETWEEN, step);
        }
        Some(None) => after(RESTING, measure),
        None => finish(),
    }
}

fn measure() {
    let done = CHECK.with_borrow_mut(|check| {
        let check = check.as_mut()?;
        let figure = memory::settled_megabytes().unwrap_or(0);
        check.figures.push(figure);
        log::info!("the memory check: round {}, {figure} MB", check.figures.len());
        Some(check.figures.len() >= ROUNDS)
    });
    match done {
        Some(false) => after(BETWEEN, step),
        _ => finish(),
    }
}

/// What `<file>.memory` holds: each round's figure, and how much the rounds after the first added.
#[must_use]
pub fn report(figures: &[u64]) -> String {
    let mut text = String::new();
    for (round, figure) in figures.iter().enumerate() {
        let _ = writeln!(text, "round {} MB={figure}", round + 1);
    }
    if let (Some(first), Some(last)) = (figures.first(), figures.last()) {
        let _ = writeln!(text, "grew MB={}", last.saturating_sub(*first));
    }
    text
}

fn finish() {
    let Some(check) = CHECK.take() else { return };
    let text = report(&check.figures);
    let grew = check.figures.last().zip(check.figures.first()).map_or(0, |(last, first)| last.saturating_sub(*first));
    log::info!("the memory check: {} rounds, grew {grew} MB after the first", check.figures.len());
    let _ = std::fs::write(&check.file, text);
    (check.then)();
}

#[cfg(test)]
mod tests;
