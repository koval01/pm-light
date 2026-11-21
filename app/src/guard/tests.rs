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

#[test]
fn a_contained_call_hands_back_what_it_made() {
    assert_eq!(contain("a test", || 7), Some(7));
    assert_eq!(CONTAINED.get(), 0, "a call that ended is no longer counted");
}

#[test]
fn a_panic_in_a_contained_call_ends_only_that_call() {
    let nothing: Option<()> = contain("a test", || panic!("a system's callback failed"));
    assert_eq!(nothing, None);
    assert_eq!(CONTAINED.get(), 0, "after a panic too: or every later panic on the window's thread would be kept");
    assert_eq!(contain("a test", || 1), Some(1), "the next call runs as usual");
}

#[test]
fn calls_inside_calls_are_counted() {
    let inner = contain("outer", || {
        let depth = CONTAINED.get();
        contain("inner", || CONTAINED.get()).map(|inner| (depth, inner))
    });
    assert_eq!(inner, Some(Some((1, 2))));
    assert_eq!(CONTAINED.get(), 0);
}

#[test]
fn only_a_bare_panic_on_the_window_thread_ends_the_program() {
    assert!(ends_the_program("main", 0));
    assert!(!ends_the_program("main", 1), "inside a call from the system, the call ends, not the program");
    assert!(!ends_the_program("tokio-runtime-worker", 0));
    assert!(!ends_the_program("tokio-runtime-worker", 2));
}
