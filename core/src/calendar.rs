//! The month a date picker shows and the moment being chosen in it. Weekdays go by Proton's
//! numbering, where 1 is Monday and 7 is Sunday.

use chrono::{Datelike, Days, Months, NaiveDate, NaiveDateTime, Timelike};

/// Six weeks: a month never needs more, and the same height keeps the sheet from jumping.
const CELLS: u64 = 42;
const DAYS_IN_WEEK: i32 = 7;
const HOURS_IN_DAY: i32 = 24;
const MINUTES_IN_HOUR: i32 = 60;
/// The arrows stop on round minutes, and an odd one meets the next round one first.
const MINUTE_STEP: i32 = 5;
pub const DAY_FORMAT: &str = "%Y-%m-%d";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Picking {
    pub moment: NaiveDateTime,
    /// The first of the month on show, which may not be the month of the moment.
    pub month: NaiveDate,
}

fn first_of(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

impl Picking {
    #[must_use]
    pub fn new(moment: NaiveDateTime) -> Self {
        Self { moment, month: first_of(moment.date()) }
    }

    /// The days of the grid, in order, starting on the given weekday.
    #[must_use]
    pub fn cells(&self, first_weekday: u8) -> Vec<NaiveDate> {
        let weekday = self.month.weekday().number_from_monday();
        let back = (weekday + DAYS_IN_WEEK.unsigned_abs() - u32::from(first_weekday)) % DAYS_IN_WEEK.unsigned_abs();
        let start = self.month.checked_sub_days(Days::new(u64::from(back))).unwrap_or(self.month);
        (0..CELLS).filter_map(|day| start.checked_add_days(Days::new(day))).collect()
    }

    pub fn pick(&mut self, day: NaiveDate) {
        self.moment = day.and_time(self.moment.time());
        self.month = first_of(day);
    }

    /// Goes through the months without touching what is chosen.
    pub fn step_month(&mut self, delta: i32) {
        let months = Months::new(delta.unsigned_abs());
        let moved =
            if delta < 0 { self.month.checked_sub_months(months) } else { self.month.checked_add_months(months) };
        self.month = moved.unwrap_or(self.month);
    }

    pub fn step_day(&mut self, delta: i32) {
        let date = self.moment.date();
        let days = Days::new(u64::from(delta.unsigned_abs()));
        let moved = if delta < 0 { date.checked_sub_days(days) } else { date.checked_add_days(days) };
        self.pick(moved.unwrap_or(date));
    }

    pub fn step_hour(&mut self, delta: i32) {
        let hour = i32::try_from(self.moment.hour()).unwrap_or(0) + delta;
        self.set_time(hour.rem_euclid(HOURS_IN_DAY), i32::try_from(self.moment.minute()).unwrap_or(0));
    }

    pub fn step_minute(&mut self, delta: i32) {
        let minute = i32::try_from(self.moment.minute()).unwrap_or(0);
        let next = if delta < 0 {
            (minute - 1).div_euclid(MINUTE_STEP) * MINUTE_STEP
        } else {
            (minute.div_euclid(MINUTE_STEP) + 1) * MINUTE_STEP
        };
        self.set_time(i32::try_from(self.moment.hour()).unwrap_or(0), next.rem_euclid(MINUTES_IN_HOUR));
    }

    fn set_time(&mut self, hour: i32, minute: i32) {
        let (Ok(hour), Ok(minute)) = (u32::try_from(hour), u32::try_from(minute)) else { return };
        if let Some(moment) = self.moment.date().and_hms_opt(hour, minute, 0) {
            self.moment = moment;
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
