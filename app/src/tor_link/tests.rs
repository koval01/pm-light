use super::{Words, line};
use crate::preferences::{Preferences, TorChoice};
use proton_core::tor::{Route, TorState, TorStatus};

fn words() -> Words {
    Words {
        starting: "Tor · connecting {percent}%".to_string(),
        ready: "Tor · {path}".to_string(),
        ready_alone: "Tor · connected".to_string(),
        failed: "Tor could not connect".to_string(),
    }
}

fn status(state: TorState, percent: u8, circuit: &[&str]) -> TorStatus {
    TorStatus { state, percent, circuit: circuit.iter().map(|country| (*country).to_string()).collect(), hops: Vec::new() }
}

#[test]
fn the_line_tells_how_far_tor_has_come() {
    assert_eq!(line(&status(TorState::Starting, 40, &[]), &words()), "Tor · connecting 40%");
    assert_eq!(line(&status(TorState::Starting, 0, &[]), &words()), "Tor · connecting 0%");
}

#[test]
fn the_line_names_the_countries_of_the_circuit_in_order() {
    assert_eq!(line(&status(TorState::Ready, 100, &["DE", "FR", "NL"]), &words()), "Tor · DE → FR → NL");
    assert_eq!(line(&status(TorState::Ready, 100, &["DE", "??", "NL"]), &words()), "Tor · DE → ?? → NL");
    assert_eq!(line(&status(TorState::Ready, 100, &[]), &words()), "Tor · connected");
}

#[test]
fn a_failure_is_said_as_such() {
    assert_eq!(line(&status(TorState::Failed, 30, &["DE"]), &words()), "Tor could not connect");
}

/// Someone who used a version without Tor has no word about it in their preferences, and is asked.
#[test]
fn preferences_from_before_tor_mean_not_asked_yet() {
    let before: Preferences = serde_json::from_str(r#"{"notifications": true, "zoom": 100}"#).unwrap();
    assert_eq!(before.tor, None);
    assert_eq!(Preferences::default().tor, None);
}

#[test]
fn a_choice_about_tor_is_kept_as_made() {
    for choice in [
        TorChoice { on: true, route: Route::Onion },
        TorChoice { on: true, route: Route::Exit },
        TorChoice { on: false, route: Route::Onion },
    ] {
        let preferences = Preferences { tor: Some(choice), ..Preferences::default() };
        let read: Preferences = serde_json::from_str(&serde_json::to_string(&preferences).unwrap()).unwrap();
        assert_eq!(read.tor, Some(choice));
    }
    assert_eq!(TorChoice::default(), TorChoice { on: false, route: Route::Onion });
}

#[test]
fn an_unknown_route_in_a_file_is_not_taken_for_tor_off() {
    let read: Result<Preferences, _> = serde_json::from_str(r#"{"tor": {"on": true, "route": "bridge"}}"#);
    assert!(read.is_err() || read.is_ok_and(|preferences| preferences.tor.is_some_and(|choice| choice.on)));
}

#[test]
fn a_relay_is_shown_by_country_and_address() {
    let hop = proton_core::tor::Hop { country: "DE".to_string(), address: "192.0.2.7".to_string() };
    assert_eq!(super::hop_text(&hop), "DE · 192.0.2.7");
    let unknown = proton_core::tor::Hop { country: "??".to_string(), address: String::new() };
    assert_eq!(super::hop_text(&unknown), "??");
}
