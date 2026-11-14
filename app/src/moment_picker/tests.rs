use super::*;

#[test]
fn hours_read_as_the_clock_chosen() {
    assert_eq!(hour_shown(0, false), ("00".to_string(), String::new()));
    assert_eq!(hour_shown(17, false), ("17".to_string(), String::new()));
    assert_eq!(hour_shown(0, true), ("12".to_string(), "AM".to_string()));
    assert_eq!(hour_shown(12, true), ("12".to_string(), "PM".to_string()));
    assert_eq!(hour_shown(17, true), ("05".to_string(), "PM".to_string()));
}

#[test]
fn a_field_may_hold_a_moment_or_a_day() {
    assert_eq!(moment_of("2026-12-31T09:30").map(|at| (at.hour(), at.minute())), Some((9, 30)));
    assert_eq!(moment_of("2026-12-31").map(|at| (at.day(), at.hour())), Some((31, 0)));
    assert_eq!(moment_of("31.12.2026").map(|at| at.day()), Some(31));
    assert_eq!(moment_of("soon"), None);
}

#[test]
fn what_comes_back_is_what_the_fields_read() {
    let moment = moment_of("2026-12-31T09:30").unwrap();
    assert_eq!(field_of(moment, true), "2026-12-31T09:30");
    assert_eq!(field_of(moment, false), "2026-12-31");
}
