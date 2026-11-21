//! The month a date picker shows and the moment being chosen in it. Weekdays go by Proton's
//! numbering, where 1 is Monday and 7 is Sunday.

use jiff::{
    Span,
    civil::{Date, DateTime, Time},
};

/// Six weeks: a month never needs more, and the same height keeps the sheet from jumping.
const CELLS: i64 = 42;
const DAYS_IN_WEEK: i8 = 7;
const HOURS_IN_DAY: i32 = 24;
const MINUTES_IN_HOUR: i32 = 60;
/// The arrows stop on round minutes, and an odd one meets the next round one first.
const MINUTE_STEP: i32 = 5;
pub const DAY_FORMAT: &str = "%Y-%m-%d";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picking {
    pub moment: DateTime,
    /// The first of the month on show, which may not be the month of the moment.
    pub month: Date,
}

/// A day that far from another, if the calendar goes that far.
fn days_from(date: Date, days: i64) -> Option<Date> {
    date.checked_add(Span::new().try_days(days).ok()?).ok()
}

impl Picking {
    #[must_use]
    pub fn new(moment: DateTime) -> Self {
        Self { moment, month: moment.date().first_of_month() }
    }

    /// The days of the grid, in order, starting on the given weekday. A day Proton does not number is Monday.
    #[must_use]
    pub fn cells(&self, first_weekday: u8) -> Vec<Date> {
        let weekday = self.month.weekday().to_monday_one_offset();
        let first = i8::try_from(first_weekday).ok().filter(|day| (1..=DAYS_IN_WEEK).contains(day)).unwrap_or(1);
        let back = (weekday - first).rem_euclid(DAYS_IN_WEEK);
        let start = days_from(self.month, -i64::from(back)).unwrap_or(self.month);
        (0..CELLS).filter_map(|day| days_from(start, day)).collect()
    }

    pub fn pick(&mut self, day: Date) {
        self.moment = day.to_datetime(self.moment.time());
        self.month = day.first_of_month();
    }

    /// Goes through the months without touching what is chosen.
    pub fn step_month(&mut self, delta: i32) {
        let moved = Span::new().try_months(delta).ok().and_then(|months| self.month.checked_add(months).ok());
        self.month = moved.unwrap_or(self.month);
    }

    pub fn step_day(&mut self, delta: i32) {
        let date = self.moment.date();
        self.pick(days_from(date, i64::from(delta)).unwrap_or(date));
    }

    pub fn step_hour(&mut self, delta: i32) {
        let hour = i32::from(self.moment.hour()) + delta;
        self.set_time(hour.rem_euclid(HOURS_IN_DAY), i32::from(self.moment.minute()));
    }

    pub fn step_minute(&mut self, delta: i32) {
        let minute = i32::from(self.moment.minute());
        let next = if delta < 0 {
            (minute - 1).div_euclid(MINUTE_STEP) * MINUTE_STEP
        } else {
            (minute.div_euclid(MINUTE_STEP) + 1) * MINUTE_STEP
        };
        self.set_time(i32::from(self.moment.hour()), next.rem_euclid(MINUTES_IN_HOUR));
    }

    fn set_time(&mut self, hour: i32, minute: i32) {
        let (Ok(hour), Ok(minute)) = (i8::try_from(hour), i8::try_from(minute)) else { return };
        if let Ok(time) = Time::new(hour, minute, 0, 0) {
            self.moment = self.moment.date().to_datetime(time);
        }
    }
}

/// Names listed from Monday, put in the order a week is shown.
#[must_use]
pub fn week_in_order<'a>(names: &[&'a str], first_weekday: u8) -> Vec<&'a str> {
    let first = usize::from(first_weekday).saturating_sub(1);
    (0..names.len()).filter_map(|day| names.get((first + day) % names.len()).copied()).collect()
}

#[cfg(test)]
mod tests;
