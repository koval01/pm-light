//! Times people pick by name: tomorrow morning, next week. Local to wherever they are.

use crate::error::{Error, Result};
use jiff::{
    SignedDuration, Span, Timestamp, Zoned,
    civil::{Date, DateTime, Time, Weekday},
    fmt::strtime,
    tz::{self, TimeZone},
};
use parking_lot::RwLock;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const SEND_MORNING_HOUR: i8 = 8;
const SNOOZE_MORNING_HOUR: i8 = 9;
const LATER_TODAY: SignedDuration = SignedDuration::from_hours(3);
const A_DAY: SignedDuration = SignedDuration::from_hours(24);
const HOURS_IN_DAY: i64 = 24;
const DAYS_IN_WEEK: i64 = 7;
const LAST_SECOND: Time = Time::constant(23, 59, 59, 0);
/// Names in a zone database that are no place anyone lives: the rules a POSIX zone string falls back on, and the
/// zone of a system not yet set up.
const NOT_PLACES: [&str; 2] = ["posixrules", "Factory"];
/// The format of a `datetime-local` field.
pub const FIELD_FORMAT: &str = "%Y-%m-%dT%H:%M";
const TWELVE_HOURS: &str = "%I:%M %p";
const MONDAY: u8 = 1;
const SUNDAY: u8 = 7;
const H24: u8 = 1;
const H12: u8 = 2;
const DAY_FIRST: u8 = 1;
const MONTH_FIRST: u8 = 2;
const YEAR_FIRST: u8 = 3;

/// The zone the person chose. Nothing means the system's.
static ZONE: RwLock<Option<TimeZone>> = RwLock::new(None);
/// In Proton's numbering, where 1 is Monday and 7 is Sunday.
static WEEK_START: AtomicU8 = AtomicU8::new(1);
/// As the account has it, where 0 leaves it to the language.
static WEEK_CHOSEN: AtomicU8 = AtomicU8::new(0);

/// In Proton's numbering, where 0 leaves both to the language.
static CLOCK: AtomicU8 = AtomicU8::new(0);
static DATES: AtomicU8 = AtomicU8::new(0);
/// English has its own 12 hours and month first, and the other languages here agree with each other.
static ENGLISH: AtomicBool = AtomicBool::new(false);

/// Every zone a person can choose, by its IANA name, in order: from the system's database, or from the copy built
/// into the program where the system keeps none, as on Windows.
#[must_use]
pub fn zones() -> Vec<String> {
    places(tz::db().available().map(|name| name.to_string()))
}

fn places(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = names.filter(|name| !NOT_PLACES.contains(&name.as_str())).collect();
    names.sort_unstable();
    names.dedup();
    names
}

/// An empty or unknown name is the system's zone.
pub fn set_zone(name: &str) {
    *ZONE.write() = zone_named(name);
}

fn zone_named(name: &str) -> Option<TimeZone> {
    if name.is_empty() || NOT_PLACES.contains(&name) {
        return None;
    }
    tz::db().get(name).ok()
}

/// Proton's 0 means "as the language has it", which leaves Monday here.
pub fn set_week_start(day: u8) {
    WEEK_START.store(if (1..=7).contains(&day) { day } else { 1 }, Ordering::Relaxed);
    WEEK_CHOSEN.store(day, Ordering::Relaxed);
}

pub fn set_clock_format(format: u8) {
    CLOCK.store(format, Ordering::Relaxed);
}

pub fn set_date_format(format: u8) {
    DATES.store(format, Ordering::Relaxed);
}

pub fn set_language(language: &str) {
    ENGLISH.store(language == "en", Ordering::Relaxed);
}

/// The zone times are shown in: the one chosen, or the system's.
fn zone() -> TimeZone {
    let chosen = ZONE.read().clone();
    chosen.unwrap_or_else(TimeZone::system)
}

/// Times computed the way they are shown: the moment is the same, the wall clock is the zone's.
fn now() -> Zoned {
    Timestamp::now().to_zoned(zone())
}

/// The wall clock at a moment, for text that leaves the app, such as a printed date.
#[must_use]
pub fn clock_text(seconds: i64, format: &str) -> String {
    Timestamp::from_second(seconds).map_or_else(|_| String::new(), |at| written(&at.to_zoned(zone()), format))
}

/// A moment for the journal: the date as the account writes it, and the clock to the millisecond,
/// in the zone the account shows.
#[must_use]
pub fn precise_text(milliseconds: i64) -> String {
    let (clock, dates, english) = formats();
    let format = precise_format(clock, dates, english);
    Timestamp::from_millisecond(milliseconds)
        .map_or_else(|_| String::new(), |at| written(&at.to_zoned(zone()), &format))
}

fn precise_format(clock: u8, dates: u8, english: bool) -> String {
    let twelve = clock == H12 || (clock != H24 && english);
    format!("{} {}", date_pattern(dates, english), if twelve { "%I:%M:%S%.3f %p" } else { "%H:%M:%S%.3f" })
}

/// A format from a dictionary, and a bad one must not panic while being written out.
fn written(time: &Zoned, format: &str) -> String {
    strtime::format(format, time).unwrap_or_else(|_| time.timestamp().display_with_offset(time.offset()).to_string())
}

/// Which heading a mail falls under in a list: 1 today, 2 yesterday, 3 this week, 4 earlier.
#[must_use]
pub fn age_group(seconds: i64) -> i32 {
    group(&now(), seconds)
}

/// By the calendar where the person is, not by hours: a mail from last night is yesterday's.
fn group(now: &Zoned, seconds: i64) -> i32 {
    let Ok(then) = Timestamp::from_second(seconds) else { return 4 };
    let then = then.to_zoned(now.time_zone().clone());
    match now.date().duration_since(then.date()).as_hours() / HOURS_IN_DAY {
        ..=0 => 1,
        1 => 2,
        2..=6 => 3,
        _ => 4,
    }
}

const fn clock_pattern(clock: u8, english: bool) -> &'static str {
    if clock == H12 || (clock != H24 && english) { TWELVE_HOURS } else { "%H:%M" }
}

const fn date_pattern(dates: u8, english: bool) -> &'static str {
    match dates {
        DAY_FIRST => "%d/%m/%Y",
        MONTH_FIRST => "%m/%d/%Y",
        YEAR_FIRST => "%Y-%m-%d",
        _ if english => "%m/%d/%Y",
        _ => "%d.%m.%Y",
    }
}

/// The hour today, and the date before that. A date in the order chosen is always whole.
const fn list_pattern(age: i32, clock: u8, dates: u8, english: bool) -> &'static str {
    match (age, dates, english) {
        (1, ..) => clock_pattern(clock, english),
        (_, DAY_FIRST | MONTH_FIRST | YEAR_FIRST, _) => date_pattern(dates, english),
        (2 | 3, _, true) => "%m/%d",
        (2 | 3, _, false) => "%d.%m",
        (_, _, true) => "%m/%d/%y",
        (_, _, false) => "%d.%m.%y",
    }
}

fn formats() -> (u8, u8, bool) {
    (CLOCK.load(Ordering::Relaxed), DATES.load(Ordering::Relaxed), ENGLISH.load(Ordering::Relaxed))
}

/// A time on a list: numbers need no translation.
#[must_use]
pub fn list_time(seconds: i64) -> String {
    let (clock, dates, english) = formats();
    clock_text(seconds, list_pattern(age_group(seconds), clock, dates, english))
}

/// Where a calendar's week starts. English begins on Sunday, unlike the others here.
pub fn first_day_of_week() -> u8 {
    match (WEEK_CHOSEN.load(Ordering::Relaxed), ENGLISH.load(Ordering::Relaxed)) {
        (day @ 1..=7, _) => day,
        (_, true) => SUNDAY,
        (_, false) => MONDAY,
    }
}

#[must_use]
pub fn uses_twelve_hours() -> bool {
    let (clock, _, english) = formats();
    clock_pattern(clock, english) == TWELVE_HOURS
}

/// How long until Proton deletes a mail by itself, as the web client words it.
///
/// Whole days to the end of that day, or less than one when under 24 hours are left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    LessThanADay,
    Days(i64),
}

/// `None` when the mail does not expire, or already has.
#[must_use]
pub fn left_until(expires_seconds: u64) -> Option<Left> {
    let expires = i64::try_from(expires_seconds).ok().filter(|seconds| *seconds > 0)?;
    left_at(&now(), expires)
}

fn left_at(now: &Zoned, expires: i64) -> Option<Left> {
    let then = Timestamp::from_second(expires).ok()?.to_zoned(now.time_zone().clone());
    if then <= *now {
        return None;
    }
    if now.duration_until(&then) < A_DAY {
        return Some(Left::LessThanADay);
    }
    let end_of_day = then.date().to_datetime(LAST_SECOND);
    let end = now.time_zone().to_ambiguous_zoned(end_of_day).earlier().ok()?;
    Some(Left::Days(now.duration_until(&end).as_hours() / HOURS_IN_DAY))
}

#[must_use]
pub fn today() -> Date {
    now().date()
}

/// A moment that is still only being chosen, and so belongs to no zone yet.
#[must_use]
pub fn wall_text(moment: DateTime, with_time: bool) -> String {
    let (clock, dates, english) = formats();
    let date = date_pattern(dates, english);
    let format = if with_time { format!("{date} {}", clock_pattern(clock, english)) } else { date.to_string() };
    strtime::format(&format, moment).unwrap_or_else(|_| moment.to_string())
}

#[must_use]
pub fn hour_text(seconds: i64) -> String {
    let (clock, _, english) = formats();
    clock_text(seconds, clock_pattern(clock, english))
}

#[must_use]
pub fn day_text(seconds: i64) -> String {
    let (_, dates, english) = formats();
    clock_text(seconds, date_pattern(dates, english))
}

#[must_use]
pub fn moment_text(seconds: i64) -> String {
    let (clock, dates, english) = formats();
    clock_text(seconds, &format!("{} {}", date_pattern(dates, english), clock_pattern(clock, english)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnoozeChoice {
    Later,
    Tomorrow,
    NextWeek,
}

fn morning_in(now: &Zoned, days: i64, hour: i8) -> Option<DateTime> {
    let day = now.date().checked_add(Span::new().try_days(days).ok()?).ok()?;
    Some(day.to_datetime(Time::new(hour, 0, 0, 0).ok()?))
}

/// A wall-clock time the clocks skipped moves on by the length of the change, as 02:30 becomes 03:30 in spring, and
/// one they show twice in autumn is the first of the two.
fn resolve(zone: &TimeZone, at: DateTime) -> Option<Zoned> {
    zone.to_ambiguous_zoned(at).compatible().ok()
}

fn snooze_at(choice: SnoozeChoice, now: &Zoned, week_start: u8) -> Option<i64> {
    let morning = |days| resolve(now.time_zone(), morning_in(now, days, SNOOZE_MORNING_HOUR)?);
    let at = match choice {
        SnoozeChoice::Later => now.checked_add(LATER_TODAY).ok()?,
        SnoozeChoice::Tomorrow => morning(1)?,
        SnoozeChoice::NextWeek => {
            let first = i8::try_from(week_start).ok().and_then(|day| Weekday::from_monday_one_offset(day).ok());
            let until_it = i64::from(first.unwrap_or(Weekday::Monday).since(now.weekday()));
            morning(if until_it == 0 { DAYS_IN_WEEK } else { until_it })?
        }
    };
    Some(at.timestamp().as_second())
}

/// Seconds since the epoch. "Next week" means the coming first day of the week.
pub fn snooze_time(choice: SnoozeChoice) -> Result<u64> {
    let week_start = WEEK_START.load(Ordering::Relaxed);
    let at = snooze_at(choice, &now(), week_start).ok_or(Error::BadTime)?;
    u64::try_from(at).map_err(|_| Error::BadTime)
}

fn tomorrow_field(now: &Zoned) -> String {
    morning_in(now, 1, SEND_MORNING_HOUR)
        .map_or_else(String::new, |at| strtime::format(FIELD_FORMAT, at).unwrap_or_default())
}

/// What a `datetime-local` field should hold for "tomorrow morning".
#[must_use]
pub fn tomorrow_morning() -> String {
    tomorrow_field(&now())
}

fn scheduled_at(value: &str, now: &Zoned) -> Result<i64> {
    let at = DateTime::strptime(FIELD_FORMAT, value).map_err(|_| Error::BadTime)?;
    let at = resolve(now.time_zone(), at).ok_or(Error::BadTime)?;
    if at <= *now {
        return Err(Error::PastTime);
    }
    Ok(at.timestamp().as_second())
}

/// A moment as a date field shows it, in the zone in use.
#[must_use]
pub fn field_text(seconds: u64) -> String {
    clock_text(i64::try_from(seconds).unwrap_or(0), FIELD_FORMAT)
}

/// What a date field holds, for a moment that may be in the past.
pub fn field_time(value: &str) -> Result<u64> {
    let at = DateTime::strptime(FIELD_FORMAT, value).map_err(|_| Error::BadTime)?;
    let at = resolve(&zone(), at).ok_or(Error::BadTime)?;
    u64::try_from(at.timestamp().as_second()).map_err(|_| Error::BadTime)
}

#[must_use]
pub fn now_seconds() -> i64 {
    Timestamp::now().as_second()
}

/// When to send, from what the field holds. A scheduled send must be in the future.
pub fn scheduled_time(value: &str) -> Result<u64> {
    u64::try_from(scheduled_at(value, &now())?).map_err(|_| Error::BadTime)
}

#[cfg(test)]
mod tests;
