use super::*;

const SECOND: Duration = Duration::from_secs(1);

#[test]
fn stays_quiet_between_periods() {
    assert!(!due(HEARTBEAT, 10 * SECOND));
}

#[test]
fn fires_every_period() {
    assert!(due(HEARTBEAT, PERIOD));
}

#[test]
fn a_late_heartbeat_means_the_machine_woke_up() {
    assert!(due(WAKE_GAP, SECOND));
}

#[test]
fn a_clock_set_backwards_is_not_a_wake() {
    assert_eq!(elapsed(SystemTime::now() + 3600 * SECOND), Duration::ZERO);
}

#[test]
fn a_link_is_news_only_when_it_changes() {
    let mut known = Link::Online;
    assert_eq!(change_link(&mut known, Link::Online), None);
    assert_eq!(change_link(&mut known, Link::Unavailable), Some(Link::Unavailable));
    assert_eq!(change_link(&mut known, Link::Unavailable), None);
    assert_eq!(change_link(&mut known, Link::Online), Some(Link::Online));
}

#[test]
fn checks_are_spaced_out() {
    let (mut last, start) = (None, Instant::now());
    assert!(claim_check(&mut last, start));
    assert!(!claim_check(&mut last, start + SECOND));
    assert!(claim_check(&mut last, start + MIN_INTERVAL));
}
