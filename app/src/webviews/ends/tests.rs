use super::*;

#[test]
fn webview2_kinds_each_get_what_microsoft_says_they_need() {
    let expected = [
        (0, Some(Remedy::Remake)), // browser process: every webview of it is closed
        (1, Some(Remedy::Reload)), // main frame's renderer: an error page stands in its place
        (2, Some(Remedy::Remake)), // renderer hangs: a reload may land in the same process
        (3, Some(Remedy::Reload)), // a frame's renderer
        (4, None),                 // utility
        (5, None),                 // sandbox helper
        (6, None),                 // GPU
        (7, None),                 // no longer used
        (8, None),                 // no longer used
        (9, None),                 // unknown process, not fatal
        (10, None),                // a kind added later
        (-1, None),
    ];
    for (kind, remedy) in expected {
        assert_eq!(on_windows(Some(kind)), remedy, "WebView2 kind {kind}");
    }
}

#[test]
fn a_webview2_failure_of_no_known_kind_is_taken_as_the_worst() {
    assert_eq!(on_windows(None), Some(Remedy::Remake));
}

#[test]
fn webkitgtk_reasons_reload_unless_the_app_ended_it() {
    assert_eq!(on_linux(0), Some(Remedy::Reload), "crashed");
    assert_eq!(on_linux(1), Some(Remedy::Reload), "past its memory limit");
    assert_eq!(on_linux(2), None, "ended by the app's own call, on purpose");
    assert_eq!(on_linux(3), Some(Remedy::Reload), "a reason added later still leaves the page empty");
    assert_eq!(on_linux(-1), Some(Remedy::Reload));
}

#[test]
fn a_page_that_ends_its_engine_every_time_is_given_up() {
    let mut tries = Tries::new();
    let start = Instant::now();
    for attempt in 1..=MOST_TRIES {
        let at = start + Duration::from_secs(u64::from(attempt));
        assert!(tries.again(7, at), "end {attempt} of {MOST_TRIES} is tried again");
    }
    let next = start + Duration::from_secs(u64::from(MOST_TRIES) + 1);
    assert!(!tries.again(7, next), "past the limit the page is given up, not loaded into a loop");
    assert!(!tries.again(7, next + Duration::from_millis(1)), "and stays given up while ends keep coming");
}

#[test]
fn another_page_is_tried_afresh() {
    let mut tries = Tries::new();
    let now = Instant::now();
    for _ in 0..=MOST_TRIES {
        tries.again(1, now);
    }
    assert!(!tries.again(1, now));
    assert!(tries.again(2, now), "a new mail is not blamed for the one before it");
    assert!(tries.again(1, now), "nor is the first mail opened again");
}

#[test]
fn ends_far_apart_are_no_loop() {
    let mut tries = Tries::new();
    let mut at = Instant::now();
    for round in 0..10 {
        assert!(tries.again(3, at), "an end after a quiet stretch is tried again, round {round}");
        at += QUIET;
    }
}

#[test]
fn ends_just_inside_the_quiet_stretch_are_counted() {
    let mut tries = Tries::new();
    let mut at = Instant::now();
    let close = QUIET.saturating_sub(Duration::from_secs(1));
    for _ in 0..MOST_TRIES {
        assert!(tries.again(3, at));
        at += close;
    }
    assert!(!tries.again(3, at), "a page that ends the engine every few minutes is still a loop");
}

thread_local! {
    static TOLD: Cell<Option<Remedy>> = const { Cell::new(None) };
}

fn told(remedy: Remedy) {
    TOLD.set(Some(remedy));
}

static TEST_KEEPER: Keeper = Keeper { what: "a test", ended: told };

#[test]
fn the_owner_hears_of_an_end_only_while_its_webview_lives() {
    let number = number();
    hand_over(&TEST_KEEPER, number, Remedy::Reload);
    assert_eq!(TOLD.take(), None, "a webview not yet made, or already gone, is no one's to bring back");
    alive(number);
    hand_over(&TEST_KEEPER, number, Remedy::Remake);
    assert_eq!(TOLD.take(), Some(Remedy::Remake));
    gone(number);
    hand_over(&TEST_KEEPER, number, Remedy::Reload);
    assert_eq!(TOLD.take(), None, "an end of the webview made before is late, and must not touch the new one");
}

#[test]
fn every_webview_gets_a_number_of_its_own() {
    let (first, second) = (number(), number());
    assert_ne!(first, second);
    alive(first);
    alive(second);
    gone(first);
    assert!(!is_alive(first));
    assert!(is_alive(second), "one webview going takes no other with it");
    gone(second);
}
