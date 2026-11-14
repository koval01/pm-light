//! A watch on the window's thread, for finding what makes animations jerk: started with `PML_WATCH_STALLS` set, a
//! timer that should fire every frame notes each time it came late, and by how much. Off otherwise, since a timer at
//! every frame keeps the processor awake.

use slint::{Timer, TimerMode};
use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};

const FRAME: Duration = Duration::from_millis(16);
/// Late by more than three frames: an animation visibly stops.
const STALL: Duration = Duration::from_millis(50);

thread_local! {
    static WATCH: RefCell<Option<Timer>> = const { RefCell::new(None) };
}

/// How long the thread was held, when a beat that was due after `FRAME` came after `between`.
pub fn held(between: Duration) -> Option<Duration> {
    let late = between.saturating_sub(FRAME);
    (late > STALL).then_some(late)
}

pub fn start() {
    if std::env::var_os("PML_WATCH_STALLS").is_none() {
        return;
    }
    let last = Cell::new(Instant::now());
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, FRAME, move || {
        let now = Instant::now();
        if let Some(late) = held(now - last.get()) {
            log::info!("the window's thread was held for {} ms", late.as_millis());
        }
        last.set(now);
    });
    WATCH.set(Some(timer));
    log::info!("watching the window's thread");
}

#[cfg(test)]
mod tests;
