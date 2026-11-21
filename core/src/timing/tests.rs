use super::*;
use jiff::{civil::date, tz::Offset};

fn berlin() -> TimeZone {
    TimeZone::get("Europe/Berlin").unwrap()
}

/// A wall-clock time at a fixed offset from UTC, in hours.
fn at(local: &str, hours: i8) -> Zoned {
    local.parse::<DateTime>().unwrap().to_zoned(TimeZone::fixed(Offset::constant(hours))).unwrap()
}

/// The wall clock two hours east of UTC, where the snooze tests are set.
fn local(seconds: i64) -> DateTime {
    Timestamp::from_second(seconds).unwrap().to_zoned(TimeZone::fixed(Offset::constant(2))).datetime()
}

/// As the web client counts: whole days to the end of the day it expires on, under 24 hours as less than one,
/// and nothing once it has passed.
#[test]
fn days_left_are_counted_to_the_end_of_the_day() {
    let zone = berlin();
    let now = date(2026, 11, 7).at(21, 0, 0, 0).to_zoned(zone.clone()).unwrap();
    let at = |y, m, d, h| date(y, m, d).at(h, 0, 0, 0).to_zoned(zone.clone()).unwrap().timestamp().as_second();
    assert_eq!(left_at(&now, at(2026, 12, 5, 21)), Some(Left::Days(28)));
    assert_eq!(left_at(&now, at(2026, 12, 5, 1)), Some(Left::Days(28)));
    assert_eq!(left_at(&now, at(2026, 11, 9, 10)), Some(Left::Days(2)));
    assert_eq!(left_at(&now, at(2026, 11, 8, 20)), Some(Left::LessThanADay));
    assert_eq!(left_at(&now, at(2026, 11, 7, 23)), Some(Left::LessThanADay));
    assert_eq!(left_at(&now, at(2026, 11, 7, 20)), None);
    assert_eq!(left_at(&now, now.timestamp().as_second()), None);
    assert_eq!(left_at(&now, i64::MAX), None);
    assert_eq!(left_until(0), None);
}

#[test]
fn the_journal_shows_seconds_and_milliseconds() {
    let at = Timestamp::from_millisecond(1_700_000_000_123).unwrap().to_zoned(TimeZone::UTC);
    assert_eq!(written(&at, &precise_format(H24, DAY_FIRST, false)), "14/11/2023 22:13:20.123");
    assert!(written(&at, &precise_format(H12, DAY_FIRST, true)).ends_with("10:13:20.123 PM"));
}

/// Every pattern the app writes, on a day and an hour that need their leading zeros.
#[test]
fn every_pattern_writes_as_people_read_it() {
    let morning = date(2026, 3, 5).at(9, 7, 0, 0).to_zoned(berlin()).unwrap();
    let written = |format: &str| written(&morning, format);
    assert_eq!(written(clock_pattern(H24, true)), "09:07");
    assert_eq!(written(clock_pattern(H12, false)), "09:07 AM");
    assert_eq!(written(date_pattern(DAY_FIRST, true)), "05/03/2026");
    assert_eq!(written(date_pattern(MONTH_FIRST, false)), "03/05/2026");
    assert_eq!(written(date_pattern(YEAR_FIRST, false)), "2026-03-05");
    assert_eq!(written(date_pattern(0, false)), "05.03.2026");
    assert_eq!(written(list_pattern(3, 0, 0, true)), "03/05");
    assert_eq!(written(list_pattern(3, 0, 0, false)), "05.03");
    assert_eq!(written(list_pattern(4, 0, 0, true)), "03/05/26");
    assert_eq!(written(list_pattern(4, 0, 0, false)), "05.03.26");
    assert_eq!(written(FIELD_FORMAT), "2026-03-05T09:07");
    assert_eq!(written("%d.%m.%Y %H:%M"), "05.03.2026 09:07", "the printed date, as the German dictionary has it");
}

#[test]
fn twelve_hours_go_round_at_noon_and_midnight() {
    let hour = |hour| written(&date(2026, 3, 5).at(hour, 30, 0, 0).to_zoned(berlin()).unwrap(), TWELVE_HOURS);
    assert_eq!(hour(0), "12:30 AM");
    assert_eq!(hour(11), "11:30 AM");
    assert_eq!(hour(12), "12:30 PM");
    assert_eq!(hour(23), "11:30 PM");
}

/// The printed date's format comes from a dictionary, and a broken one must not take the app down.
#[test]
fn a_broken_format_writes_the_moment_whole_instead() {
    let at = date(2026, 3, 5).at(9, 7, 0, 0).to_zoned(berlin()).unwrap();
    assert_eq!(written(&at, "%"), "2026-03-05T09:07:00+01:00");
    assert_eq!(written(&at, "%d.%m.%Y %"), "2026-03-05T09:07:00+01:00");
    assert_eq!(clock_text(i64::MAX, "%Y"), "");
    assert_eq!(precise_text(i64::MAX), "");
}

/// Spring forward skips 02:00 to 03:00 in Berlin on 28 March 2027; autumn shows 02:00 to 03:00 twice on
/// 25 October 2026.
#[test]
fn a_skipped_hour_moves_on_and_a_repeated_one_is_the_first() {
    let zone = berlin();
    let skipped = resolve(&zone, date(2027, 3, 28).at(2, 30, 0, 0)).unwrap();
    assert_eq!((skipped.datetime(), skipped.offset()), (date(2027, 3, 28).at(3, 30, 0, 0), Offset::constant(2)));
    let twice = resolve(&zone, date(2026, 10, 25).at(2, 30, 0, 0)).unwrap();
    assert_eq!((twice.datetime(), twice.offset()), (date(2026, 10, 25).at(2, 30, 0, 0), Offset::constant(2)));
    let field = resolve(&zone, "2027-03-28T02:30".parse().unwrap()).unwrap();
    assert_eq!(field.timestamp(), skipped.timestamp());
}

#[test]
fn a_send_in_the_skipped_hour_is_still_taken() {
    let now = date(2027, 3, 27).at(12, 0, 0, 0).to_zoned(berlin()).unwrap();
    let sent = scheduled_at("2027-03-28T02:30", &now).unwrap();
    assert_eq!(sent, "2027-03-28T01:30:00Z".parse::<Timestamp>().unwrap().as_second());
}

#[test]
fn later_is_three_hours_on() {
    let now = at("2026-11-02T10:15:00", 2);
    assert_eq!(snooze_at(SnoozeChoice::Later, &now, 1), Some(now.timestamp().as_second() + 3 * 3600));
}

#[test]
fn tomorrow_is_nine_in_the_morning_where_the_person_is() {
    let now = at("2026-11-02T23:30:00", 2);
    let target = local(snooze_at(SnoozeChoice::Tomorrow, &now, 1).unwrap());
    assert_eq!((target.day(), target.hour(), target.minute()), (3, 9, 0));
}

#[test]
fn next_week_is_the_coming_monday() {
    for (today, day) in [("2026-11-02", 9), ("2026-11-04", 9), ("2026-11-07", 9), ("2026-11-08", 9)] {
        let now = at(&format!("{today}T12:00:00"), 2);
        let target = local(snooze_at(SnoozeChoice::NextWeek, &now, 1).unwrap());
        assert_eq!((target.weekday(), target.day()), (Weekday::Monday, day), "from {today}");
    }
}

#[test]
fn the_field_holds_tomorrow_at_eight() {
    assert_eq!(tomorrow_field(&at("2026-12-31T22:00:00", 2)), "2027-01-01T08:00");
}

#[test]
fn a_time_ahead_is_accepted() {
    let now = at("2026-11-02T10:00:00", 2);
    assert_eq!(scheduled_at("2026-11-02T11:00", &now).unwrap(), now.timestamp().as_second() + 3600);
}

#[test]
fn nonsense_is_refused_and_the_past_named() {
    let now = at("2026-11-02T10:00:00", 2);
    assert!(matches!(scheduled_at("soon", &now), Err(Error::BadTime)));
    assert!(matches!(scheduled_at("", &now), Err(Error::BadTime)));
    assert!(matches!(scheduled_at("2026-11-02T11:00 ", &now), Err(Error::BadTime)));
    assert!(matches!(scheduled_at("2026-11-31T11:00", &now), Err(Error::BadTime)));
    assert!(matches!(scheduled_at("2026-11-02T24:00", &now), Err(Error::BadTime)));
    assert!(matches!(scheduled_at("2026-11-02T09:00", &now), Err(Error::PastTime)));
    assert!(matches!(scheduled_at("2026-11-02T10:00", &now), Err(Error::PastTime)));
    assert!(matches!(field_time("next tuesday"), Err(Error::BadTime)));
}

#[test]
fn next_week_follows_the_chosen_first_day() {
    let wednesday = at("2026-11-04T12:00:00", 0);
    let sunday = snooze_at(SnoozeChoice::NextWeek, &wednesday, 7).unwrap();
    assert_eq!(strtime::format("%A %d", local(sunday)).unwrap(), "Sunday 08");
    let saturday = snooze_at(SnoozeChoice::NextWeek, &wednesday, 6).unwrap();
    assert_eq!(strtime::format("%A %d", local(saturday)).unwrap(), "Saturday 07");
    let unknown = snooze_at(SnoozeChoice::NextWeek, &wednesday, 0).unwrap();
    assert_eq!(local(unknown).weekday(), Weekday::Monday, "a day Proton does not number is Monday");
}

#[test]
fn a_morning_in_a_zone_is_that_zones_morning() {
    let zone = TimeZone::get("Asia/Tokyo").unwrap();
    let now = at("2026-11-04T12:00:00", 0).with_time_zone(zone.clone());
    let tomorrow = snooze_at(SnoozeChoice::Tomorrow, &now, 1).unwrap();
    assert_eq!(Timestamp::from_second(tomorrow).unwrap().to_zoned(zone).hour(), SNOOZE_MORNING_HOUR);
}

/// By the calendar where the person is: a mail from late last night is yesterday's, though under a day old.
#[test]
fn a_list_groups_mail_by_the_day_it_came() {
    let now = date(2026, 11, 7).at(0, 30, 0, 0).to_zoned(berlin()).unwrap();
    let at = |y, m, d, h| date(y, m, d).at(h, 0, 0, 0).to_zoned(berlin()).unwrap().timestamp().as_second();
    assert_eq!(group(&now, at(2026, 11, 7, 0)), 1);
    assert_eq!(group(&now, at(2026, 11, 8, 12)), 1, "a date ahead is today's");
    assert_eq!(group(&now, at(2026, 11, 6, 23)), 2);
    assert_eq!(group(&now, at(2026, 11, 5, 12)), 3);
    assert_eq!(group(&now, at(2026, 11, 1, 12)), 3);
    assert_eq!(group(&now, at(2026, 10, 31, 12)), 4);
    assert_eq!(group(&now, i64::MAX), 4);
}

#[test]
fn every_zone_offered_is_a_place_that_loads() {
    let zones = zones();
    assert!(zones.len() > 300, "only {} zones", zones.len());
    for name in ["Europe/Berlin", "America/New_York", "Asia/Tokyo", "Australia/Lord_Howe", "UTC"] {
        assert!(zones.iter().any(|zone| zone == name), "{name} is missing");
    }
    assert!(zones.windows(2).all(|pair| pair[0] < pair[1]), "in order, and each once");
    assert!(zones.iter().all(|zone| !NOT_PLACES.contains(&zone.as_str())));
    for name in &zones {
        assert!(zone_named(name).is_some(), "{name} does not load");
    }
}

#[test]
fn a_list_of_places_leaves_out_what_is_no_place() {
    let names = ["Factory", "Europe/Berlin", "posixrules", "Asia/Tokyo", "Europe/Berlin"].map(String::from);
    assert_eq!(places(names.into_iter()), ["Asia/Tokyo", "Europe/Berlin"]);
}

#[test]
fn an_empty_or_unknown_zone_is_the_systems() {
    for name in ["", "Atlantis/Capital", "posixrules", "Factory", "../../../etc/passwd", "/etc/localtime"] {
        assert!(zone_named(name).is_none(), "{name:?}");
    }
    let tokyo = zone_named("Asia/Tokyo").unwrap();
    assert_eq!(tokyo.iana_name(), Some("Asia/Tokyo"));
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
