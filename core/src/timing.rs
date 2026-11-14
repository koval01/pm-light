//! Times people pick by name: tomorrow morning, next week. Local to wherever they are.

use crate::error::{Error, Result};
use chrono::{DateTime, Datelike, Days, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use parking_lot::RwLock;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

const SEND_MORNING_HOUR: u32 = 8;
const SNOOZE_MORNING_HOUR: u32 = 9;
const LATER_TODAY_HOURS: i64 = 3;
const DAYS_IN_WEEK: u32 = 7;
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
static ZONE: RwLock<Option<Tz>> = RwLock::new(None);
/// In Proton's numbering, where 1 is Monday and 7 is Sunday.
static WEEK_START: AtomicU8 = AtomicU8::new(1);
/// As the account has it, where 0 leaves it to the language.
static WEEK_CHOSEN: AtomicU8 = AtomicU8::new(0);

/// In Proton's numbering, where 0 leaves both to the language.
static CLOCK: AtomicU8 = AtomicU8::new(0);
static DATES: AtomicU8 = AtomicU8::new(0);
/// English has its own 12 hours and month first, and the other languages here agree with each other.
static ENGLISH: AtomicBool = AtomicBool::new(false);

/// An empty or unknown name is the system's zone.
/// Every zone a person can choose, by its IANA name.
#[must_use]
pub fn zones() -> Vec<&'static str> {
    chrono_tz::TZ_VARIANTS.iter().map(|zone| zone.name()).collect()
}

pub fn set_zone(name: &str) {
    *ZONE.write() = name.parse().ok();
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

/// Times computed the way they are shown: the moment is the same, the wall clock is the zone's.
macro_rules! in_zone {
    (|$now:ident| $work:expr) => {{
        let zone = *ZONE.read();
        match zone {
            Some(zone) => {
                let $now = Utc::now().with_timezone(&zone);
                $work
            }
            None => {
                let $now = Local::now();
                $work
            }
        }
    }};
}

/// The wall clock at a moment, for text that leaves the app, such as a printed date.
pub fn clock_text(seconds: i64, format: &str) -> String {
    let zone = *ZONE.read();
    zone.map_or_else(
        || Local.timestamp_opt(seconds, 0).single().map(|at| written(&at, format)),
        |zone| zone.timestamp_opt(seconds, 0).single().map(|at| written(&at, format)),
    )
    .unwrap_or_default()
}

/// A moment for the journal: the date as the account writes it, and the clock to the millisecond,
/// in the zone the account shows.
#[must_use]
pub fn precise_text(milliseconds: i64) -> String {
    let (clock, dates, english) = formats();
    let format = precise_format(clock, dates, english);
    let zone = *ZONE.read();
    zone.map_or_else(
        || Local.timestamp_millis_opt(milliseconds).single().map(|at| written(&at, &format)),
        |zone| zone.timestamp_millis_opt(milliseconds).single().map(|at| written(&at, &format)),
    )
    .unwrap_or_default()
}

fn precise_format(clock: u8, dates: u8, english: bool) -> String {
    let twelve = clock == H12 || (clock != H24 && english);
    format!("{} {}", date_pattern(dates, english), if twelve { "%I:%M:%S%.3f %p" } else { "%H:%M:%S%.3f" })
}

/// A format from a dictionary, and a bad one must not panic while being written out.
fn written<Z: TimeZone>(time: &DateTime<Z>, format: &str) -> String
where
    Z::Offset: std::fmt::Display,
{
    let mut text = String::new();
    if std::fmt::Write::write_fmt(&mut text, format_args!("{}", time.format(format))).is_err() {
        return time.to_rfc3339();
    }
    text
}

/// Which heading a mail falls under in a list: 1 today, 2 yesterday, 3 this week, 4 earlier.
pub fn age_group(seconds: i64) -> i32 {
    fn group<Z: TimeZone>(zone: &Z, now: &DateTime<Z>, seconds: i64) -> i32 {
        let Some(then) = zone.timestamp_opt(seconds, 0).single() else { return 4 };
        match (now.date_naive() - then.date_naive()).num_days() {
            ..=0 => 1,
            1 => 2,
            2..=6 => 3,
            _ => 4,
        }
    }
    in_zone!(|now| group(&now.timezone(), &now, seconds))
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
    in_zone!(|now| left_at(&now, expires))
}

fn left_at<Z: TimeZone>(now: &DateTime<Z>, expires: i64) -> Option<Left> {
    let then = now.timezone().timestamp_opt(expires, 0).single()?;
    if then <= *now {
        return None;
    }
    if then.clone() - now.clone() < Duration::hours(24) {
        return Some(Left::LessThanADay);
    }
    let end_of_day = then.date_naive().and_hms_opt(23, 59, 59)?;
    let end = now.timezone().from_local_datetime(&end_of_day).earliest()?;
    Some(Left::Days((end - now.clone()).num_days()))
}

pub fn today() -> NaiveDate {
    in_zone!(|now| now.date_naive())
}

/// A moment that is still only being chosen, and so belongs to no zone yet.
#[must_use]
pub fn wall_text(moment: NaiveDateTime, with_time: bool) -> String {
    let (clock, dates, english) = formats();
    let date = date_pattern(dates, english);
    let format = if with_time { format!("{date} {}", clock_pattern(clock, english)) } else { date.to_string() };
    let mut text = String::new();
    if std::fmt::Write::write_fmt(&mut text, format_args!("{}", moment.format(&format))).is_err() {
        return moment.to_string();
    }
    text
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

fn morning_in<Z: TimeZone>(now: &DateTime<Z>, days: u32, hour: u32) -> Option<NaiveDateTime> {
    now.date_naive().checked_add_days(Days::new(u64::from(days)))?.and_hms_opt(hour, 0, 0)
}

/// A wall-clock time the clocks skipped takes the moment after the change.
fn resolve<Z: TimeZone>(zone: &Z, at: NaiveDateTime) -> Option<DateTime<Z>> {
    match zone.from_local_datetime(&at) {
        LocalResult::Single(time) | LocalResult::Ambiguous(time, _) => Some(time),
        LocalResult::None => zone.from_local_datetime(&(at + Duration::hours(1))).earliest(),
    }
}

fn snooze_at<Z: TimeZone>(choice: SnoozeChoice, now: &DateTime<Z>, week_start: u8) -> Option<i64> {
    let morning = |days| resolve(&now.timezone(), morning_in(now, days, SNOOZE_MORNING_HOUR)?);
    let at = match choice {
        SnoozeChoice::Later => now.clone() + Duration::hours(LATER_TODAY_HOURS),
        SnoozeChoice::Tomorrow => morning(1)?,
        SnoozeChoice::NextWeek => {
            let today = now.weekday().num_days_from_sunday();
            let first = u32::from(week_start) % DAYS_IN_WEEK;
            let until_it = (DAYS_IN_WEEK + first - today) % DAYS_IN_WEEK;
            morning(if until_it == 0 { DAYS_IN_WEEK } else { until_it })?
        }
    };
    Some(at.timestamp())
}

/// Seconds since the epoch. "Next week" means the coming first day of the week.
pub fn snooze_time(choice: SnoozeChoice) -> Result<u64> {
    let week_start = WEEK_START.load(Ordering::Relaxed);
    let at = in_zone!(|now| snooze_at(choice, &now, week_start)).ok_or(Error::BadTime)?;
    u64::try_from(at).map_err(|_| Error::BadTime)
}

fn tomorrow_field<Z: TimeZone>(now: &DateTime<Z>) -> String {
    morning_in(now, 1, SEND_MORNING_HOUR).map_or_else(String::new, |at| at.format(FIELD_FORMAT).to_string())
}

/// What a `datetime-local` field should hold for "tomorrow morning".
pub fn tomorrow_morning() -> String {
    in_zone!(|now| tomorrow_field(&now))
}

fn scheduled_at<Z: TimeZone>(value: &str, now: &DateTime<Z>) -> Result<i64> {
    let at = NaiveDateTime::parse_from_str(value, FIELD_FORMAT).map_err(|_| Error::BadTime)?;
    let at = resolve(&now.timezone(), at).ok_or(Error::BadTime)?;
    if at <= *now {
        return Err(Error::PastTime);
    }
    Ok(at.timestamp())
}

/// A moment as a date field shows it, in the zone in use.
#[must_use]
pub fn field_text(seconds: u64) -> String {
    clock_text(i64::try_from(seconds).unwrap_or(0), FIELD_FORMAT)
}

/// What a date field holds, for a moment that may be in the past.
pub fn field_time(value: &str) -> Result<u64> {
    let at = NaiveDateTime::parse_from_str(value, FIELD_FORMAT).map_err(|_| Error::BadTime)?;
    let seconds = in_zone!(|now| resolve(&now.timezone(), at).map(|at| at.timestamp()));
    seconds.and_then(|seconds| u64::try_from(seconds).ok()).ok_or(Error::BadTime)
}

#[must_use]
pub fn now_seconds() -> i64 {
    Utc::now().timestamp()
}

/// When to send, from what the field holds. A scheduled send must be in the future.
pub fn scheduled_time(value: &str) -> Result<u64> {
    u64::try_from(in_zone!(|now| scheduled_at(value, &now))?).map_err(|_| Error::BadTime)
}

#[cfg(test)]
mod tests;
