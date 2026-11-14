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
