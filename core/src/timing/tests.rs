use super::*;
use chrono::{FixedOffset, Timelike};

/// As the web client counts: whole days to the end of the day it expires on, under 24 hours as less than one,
/// and nothing once it has passed.
#[test]
fn days_left_are_counted_to_the_end_of_the_day() {
    let zone = chrono_tz::Europe::Berlin;
    let now = zone.with_ymd_and_hms(2026, 11, 7, 21, 0, 0).single().expect("a time");
    let at = |y, m, d, h| zone.with_ymd_and_hms(y, m, d, h, 0, 0).single().expect("a time").timestamp();
    assert_eq!(left_at(&now, at(2026, 12, 5, 21)), Some(Left::Days(28)));
    assert_eq!(left_at(&now, at(2026, 12, 5, 1)), Some(Left::Days(28)));
    assert_eq!(left_at(&now, at(2026, 11, 9, 10)), Some(Left::Days(2)));
    assert_eq!(left_at(&now, at(2026, 11, 8, 20)), Some(Left::LessThanADay));
    assert_eq!(left_at(&now, at(2026, 11, 7, 23)), Some(Left::LessThanADay));
    assert_eq!(left_at(&now, at(2026, 11, 7, 20)), None);
    assert_eq!(left_at(&now, now.timestamp()), None);
    assert_eq!(left_until(0), None);
}

fn at(text: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(text).unwrap()
}

fn local(seconds: i64) -> NaiveDateTime {
    at("2026-01-01T00:00:00+02:00").timezone().timestamp_opt(seconds, 0).unwrap().naive_local()
}

#[test]
fn the_journal_shows_seconds_and_milliseconds() {
    let at = Utc.timestamp_millis_opt(1_700_000_000_123).single().unwrap();
    assert_eq!(written(&at, &precise_format(H24, DAY_FIRST, false)), "14/11/2023 22:13:20.123");
    assert!(written(&at, &precise_format(H12, DAY_FIRST, true)).ends_with("10:13:20.123 PM"));
}

#[test]
fn later_is_three_hours_on() {
    let now = at("2026-11-02T10:15:00+02:00");
    assert_eq!(snooze_at(SnoozeChoice::Later, &now, 1), Some(now.timestamp() + 3 * 3600));
}

#[test]
fn tomorrow_is_nine_in_the_morning_where_the_person_is() {
    let now = at("2026-11-02T23:30:00+02:00");
    let target = local(snooze_at(SnoozeChoice::Tomorrow, &now, 1).unwrap());
    assert_eq!((target.day(), target.hour(), target.minute()), (3, 9, 0));
}

#[test]
fn next_week_is_the_coming_monday() {
    for (today, day) in [("2026-11-02", 9), ("2026-11-04", 9), ("2026-11-07", 9), ("2026-11-08", 9)] {
        let now = at(&format!("{today}T12:00:00+02:00"));
        let target = local(snooze_at(SnoozeChoice::NextWeek, &now, 1).unwrap());
        assert_eq!((target.weekday().num_days_from_monday(), target.day()), (0, day), "from {today}");
    }
}

#[test]
fn the_field_holds_tomorrow_at_eight() {
    assert_eq!(tomorrow_field(&at("2026-12-31T22:00:00+02:00")), "2027-01-01T08:00");
}

#[test]
fn a_time_ahead_is_accepted() {
    let now = at("2026-11-02T10:00:00+02:00");
    assert_eq!(scheduled_at("2026-11-02T11:00", &now).unwrap(), now.timestamp() + 3600);
}

#[test]
fn nonsense_is_refused_and_the_past_named() {
    let now = at("2026-11-02T10:00:00+02:00");
    assert!(matches!(scheduled_at("soon", &now), Err(Error::BadTime)));
    assert!(matches!(scheduled_at("", &now), Err(Error::BadTime)));
    assert!(matches!(scheduled_at("2026-11-02T09:00", &now), Err(Error::PastTime)));
    assert!(matches!(scheduled_at("2026-11-02T10:00", &now), Err(Error::PastTime)));
}

#[test]
fn next_week_follows_the_chosen_first_day() {
    let wednesday = at("2026-11-04T12:00:00+00:00");
    let sunday = snooze_at(SnoozeChoice::NextWeek, &wednesday, 7).unwrap();
    assert_eq!(local(sunday).format("%A %d").to_string(), "Sunday 08");
    let saturday = snooze_at(SnoozeChoice::NextWeek, &wednesday, 6).unwrap();
    assert_eq!(local(saturday).format("%A %d").to_string(), "Saturday 07");
}

#[test]
fn a_morning_in_a_zone_is_that_zones_morning() {
    let zone: Tz = "Asia/Tokyo".parse().unwrap();
    let now = at("2026-11-04T12:00:00+00:00").with_timezone(&zone);
    let tomorrow = snooze_at(SnoozeChoice::Tomorrow, &now, 1).unwrap();
    assert_eq!(zone.timestamp_opt(tomorrow, 0).unwrap().hour(), SNOOZE_MORNING_HOUR);
}

#[test]
fn the_language_decides_until_a_format_is_chosen() {
    assert_eq!(clock_pattern(0, false), "%H:%M");
    assert_eq!(clock_pattern(0, true), "%I:%M %p");
    assert_eq!(date_pattern(0, false), "%d.%m.%Y");
    assert_eq!(date_pattern(0, true), "%m/%d/%Y");
}

#[test]
fn a_chosen_format_beats_the_language() {
    assert_eq!(clock_pattern(H24, true), "%H:%M");
    assert_eq!(clock_pattern(H12, false), "%I:%M %p");
    assert_eq!(date_pattern(YEAR_FIRST, true), "%Y-%m-%d");
    assert_eq!(date_pattern(DAY_FIRST, false), "%d/%m/%Y");
}

#[test]
fn a_list_shortens_a_date_only_when_the_order_is_the_language_s() {
    assert_eq!(list_pattern(1, H12, YEAR_FIRST, false), "%I:%M %p");
    assert_eq!(list_pattern(3, 0, 0, false), "%d.%m");
    assert_eq!(list_pattern(4, 0, 0, true), "%m/%d/%y");
    assert_eq!(list_pattern(3, 0, YEAR_FIRST, false), "%Y-%m-%d");
}
