use super::*;

#[test]
fn the_first_crash_restarts() {
    assert_eq!(next_restart(None, 1000).as_deref(), Some("--restarted=1:1000"));
}

#[test]
fn crashes_close_together_are_counted() {
    assert_eq!(next_restart(Some("--restarted=1:1000"), 1010).as_deref(), Some("--restarted=2:1010"));
}

#[test]
fn a_crash_loop_stops() {
    assert_eq!(next_restart(Some("--restarted=3:1000"), 1010), None);
}

#[test]
fn a_long_quiet_time_starts_the_count_again() {
    assert_eq!(next_restart(Some("--restarted=3:1000"), 2000).as_deref(), Some("--restarted=1:2000"));
}

#[test]
fn nonsense_counts_as_no_restart() {
    assert_eq!(next_restart(Some("--restarted=x"), 5).as_deref(), Some("--restarted=1:5"));
}
