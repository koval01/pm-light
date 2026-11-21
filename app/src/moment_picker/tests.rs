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
    assert_eq!(moment_of("31.12.2026").map(DateTime::day), Some(31));
    assert_eq!(moment_of("1.2.2026").map(|at| (at.day(), at.month())), Some((1, 2)));
    assert_eq!(moment_of("soon"), None);
    assert_eq!(moment_of(""), None);
    assert_eq!(moment_of("32.12.2026"), None);
    assert_eq!(moment_of("2026-02-29"), None, "not a leap year");
    assert_eq!(moment_of("2026-12-31T24:00"), None);
    assert_eq!(moment_of("2026-12-31T09:30 "), None);
}

#[test]
fn a_month_is_named_from_january() {
    let names = "Jan,Feb,Mar,Apr,May,Jun,Jul,Aug,Sep,Oct,Nov,Dec";
    assert_eq!(month_name(names, jiff::civil::date(2026, 1, 1)), "Jan");
    assert_eq!(month_name(names, jiff::civil::date(2026, 12, 1)), "Dec");
    assert_eq!(month_name("Jan,Feb", jiff::civil::date(2026, 12, 1)), "", "a short dictionary names nothing");
}

#[test]
fn what_comes_back_is_what_the_fields_read() {
    let moment = moment_of("2026-12-31T09:30").unwrap();
    assert_eq!(field_of(moment, true), "2026-12-31T09:30");
    assert_eq!(field_of(moment, false), "2026-12-31");
}
