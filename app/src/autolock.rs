//! Locks the app when it has been left alone. Only matters once an app password is set.

use crate::{App, auth, preferences};
use parking_lot::Mutex;
use proton_core::Service;
use slint::Weak;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

static AWAY_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

pub fn window_focused(focused: bool) {
    *AWAY_SINCE.lock() = (!focused).then(Instant::now);
}

/// `asleep` is how long the machine just slept: a closed laptop is away too, even if its window had
/// focus. Time asleep does not always count on the clock `Instant` reads.
fn is_due(minutes: u64, away: Option<Duration>, asleep: Duration) -> bool {
    let limit = Duration::from_secs(minutes.saturating_mul(60));
    minutes > 0 && (asleep >= limit || away.is_some_and(|away| away >= limit))
}

pub fn check(service: &Arc<Service>, app: &Weak<App>, asleep: Duration) {
    let away = AWAY_SINCE.lock().map(|since| since.elapsed());
    if service.is_signed_in() && is_due(preferences::get().auto_lock_minutes, away, asleep) && service.lock_now() {
        let service = service.clone();
        let _ = app.upgrade_in_event_loop(move |app| auth::locked(&service, &app));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn a_focused_window_stays_open_while_the_machine_is_awake() {
        assert!(!is_due(5, None, Duration::ZERO));
    }

    #[test]
    fn a_long_sleep_locks_even_a_focused_window() {
        assert!(is_due(5, None, 6 * MINUTE));
    }

    #[test]
    fn nothing_locks_when_the_setting_is_off() {
        assert!(!is_due(0, Some(600 * MINUTE), 600 * MINUTE));
    }

    #[test]
    fn leaving_the_window_counts() {
        assert!(is_due(5, Some(5 * MINUTE), Duration::ZERO));
        assert!(!is_due(5, Some(4 * MINUTE), Duration::ZERO));
    }
}
