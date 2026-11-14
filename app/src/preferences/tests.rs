use super::*;

#[test]
fn the_tour_waits_only_after_a_first_start() {
    assert!(first_start().tour_pending);
    // A file from a version without the tour: its owner has used the app, and is not shown it.
    assert!(!parse(r#"{"notifications": true, "zoom": 100}"#).tour_pending);
    assert!(!parse("not json").tour_pending);
    assert!(parse(r#"{"tourPending": true}"#).tour_pending);
}

#[test]
fn the_menu_bar_icon_is_left_out_only_on_macos() {
    assert_eq!(Preferences::default().menu_bar_icon, !cfg!(target_os = "macos"));
    assert_eq!(parse(r#"{"notifications": true}"#).menu_bar_icon, !cfg!(target_os = "macos"));
    assert!(parse(r#"{"menuBarIcon": true}"#).menu_bar_icon);
}

/// Gravatar tells a third party about the people one writes with: never on by itself.
#[test]
fn gravatar_is_off_unless_turned_on() {
    assert!(!Preferences::default().gravatar);
    assert!(!first_start().gravatar);
    assert!(!parse(r#"{"notifications": true}"#).gravatar);
    assert!(parse(r#"{"gravatar": true}"#).gravatar);
}

/// Covering private details is the default, and a file from before the setting existed keeps it.
#[test]
fn private_details_stay_covered_unless_asked() {
    assert!(Preferences::default().hide_private);
    assert!(first_start().hide_private);
    assert!(parse(r#"{"notifications": true}"#).hide_private);
    assert!(!parse(r#"{"hidePrivate": false}"#).hide_private);
}

#[test]
fn a_damaged_file_gives_the_defaults() {
    assert_eq!(parse("not json"), Preferences::default());
    assert_eq!(parse("[1,2]"), Preferences::default());
}

#[test]
fn a_file_of_an_older_build_keeps_what_it_has() {
    let preferences = parse(r#"{"autoLockMinutes": 15}"#);
    assert_eq!(preferences.auto_lock_minutes, 15);
    assert_eq!(preferences.mail_text, 1);
    assert!(preferences.notifications);
}

#[test]
fn preferences_survive_a_round_trip() {
    let preferences = Preferences { palette: 3, language: "de".into(), ..Preferences::default() };
    assert_eq!(serde_json::to_string(&preferences).ok().map(|text| parse(&text)), Some(preferences));
}
