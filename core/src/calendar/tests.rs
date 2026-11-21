use super::*;
use jiff::civil::{Weekday, date};

const MONDAY: u8 = 1;
const SUNDAY: u8 = 7;

fn picking(text: &str) -> Picking {
    Picking::new(DateTime::strptime("%Y-%m-%d %H:%M", text).unwrap())
}

fn time(picking: &Picking) -> (i8, i8) {
    (picking.moment.hour(), picking.moment.minute())
}

#[test]
fn a_grid_has_six_weeks_and_starts_on_the_chosen_day() {
    let cells = picking("2026-11-15 09:00").cells(MONDAY);
    assert_eq!(cells.len(), 42);
    assert_eq!(cells[0], date(2026, 10, 26));
    assert!(cells.iter().step_by(7).all(|day| day.weekday() == Weekday::Monday));
    let cells = picking("2026-11-15 09:00").cells(SUNDAY);
    assert_eq!(cells[0], date(2026, 11, 1));
    assert!(cells.iter().step_by(7).all(|day| day.weekday() == Weekday::Sunday));
    assert!(cells.windows(2).all(|pair| days_from(pair[0], 1) == Some(pair[1])), "the days follow each other");
}

/// Proton numbers the days from Monday as 1 to Sunday as 7. Every month of three years, so that each weekday
/// begins some month, and the first row always holds the 1st.
#[test]
fn a_grid_starts_on_any_day_of_the_week() {
    for year in 2026..=2028 {
        for month in 1..=12 {
            let first_of_month = date(year, month, 1);
            for first in 1..=7 {
                let cells = Picking::new(first_of_month.at(9, 0, 0, 0)).cells(first);
                let wanted = Weekday::from_monday_one_offset(i8::try_from(first).unwrap()).unwrap();
                assert_eq!(cells[0].weekday(), wanted, "{first_of_month} from day {first}");
                assert!(cells[0] <= first_of_month && first_of_month < days_from(cells[0], 7).unwrap());
            }
        }
    }
}

#[test]
fn a_day_proton_does_not_number_starts_the_week_on_monday() {
    for odd in [0, 8, 200] {
        let cells = picking("2026-11-15 09:00").cells(odd);
        assert_eq!((cells.len(), cells[0].weekday()), (42, Weekday::Monday), "day {odd}");
    }
}

#[test]
fn the_ends_of_the_calendar_do_not_break_the_grid() {
    let last = Picking::new(date(9999, 12, 31).at(23, 59, 0, 0));
    let cells = last.cells(MONDAY);
    assert!(cells.len() < 42 && cells.last() == Some(&date(9999, 12, 31)));
    let mut first = Picking::new(date(-9999, 1, 1).at(0, 0, 0, 0));
    first.step_day(-1);
    first.step_month(-1);
    assert_eq!((first.moment.date(), first.month), (date(-9999, 1, 1), date(-9999, 1, 1)));
}

#[test]
fn every_day_of_every_month_is_on_its_grid() {
    for month in 1..=12 {
        for first in 1..=7 {
            let shown = picking(&format!("2028-{month:02}-01 09:00"));
            let cells = shown.cells(first);
            let inside = cells.iter().filter(|day| day.month() == month).count();
            assert_eq!(inside, usize::try_from(shown.month.days_in_month()).unwrap());
        }
    }
}

#[test]
fn turning_a_month_leaves_the_choice_alone() {
    let mut picking = picking("2027-01-31 09:00");
    let chosen = picking.moment;
    picking.step_month(-1);
    assert_eq!(picking.month, date(2026, 12, 1));
    picking.step_month(2);
    assert_eq!(picking.month, date(2027, 2, 1));
    picking.step_month(-14);
    assert_eq!(picking.month, date(2025, 12, 1));
    assert_eq!(picking.moment, chosen);
}

#[test]
fn choosing_a_day_keeps_the_time_and_shows_its_month() {
    let mut picking = picking("2026-11-30 14:25");
    picking.pick(date(2026, 12, 2));
    assert_eq!(picking.moment.date(), date(2026, 12, 2));
    assert_eq!(time(&picking), (14, 25));
    assert_eq!(picking.month, date(2026, 12, 1));
}

#[test]
fn stepping_days_crosses_a_month() {
    let mut picking = picking("2026-11-30 09:00");
    picking.step_day(1);
    assert_eq!(picking.moment.date(), date(2026, 12, 1));
    picking.step_day(-7);
    assert_eq!(picking.moment.date(), date(2026, 11, 24));
    picking.step_day(-366);
    assert_eq!(picking.moment.date(), date(2025, 11, 23));
}

#[test]
fn hours_go_round_the_day_and_keep_the_date() {
    let mut picking = picking("2026-11-30 23:10");
    let date = picking.moment.date();
    picking.step_hour(1);
    assert_eq!(time(&picking), (0, 10));
    picking.step_hour(-1);
    assert_eq!(time(&picking), (23, 10));
    assert_eq!(picking.moment.date(), date);
}

#[test]
fn minutes_stop_on_round_numbers_and_go_round_the_hour() {
    let mut picking = picking("2026-11-30 09:03");
    picking.step_minute(1);
    assert_eq!(time(&picking), (9, 5));
    picking.step_minute(1);
    assert_eq!(time(&picking), (9, 10));
    picking.step_minute(-1);
    assert_eq!(time(&picking), (9, 5));

    let mut odd = self::picking("2026-11-30 09:03");
    odd.step_minute(-1);
    assert_eq!(time(&odd), (9, 0));
    odd.step_minute(-1);
    assert_eq!(time(&odd), (9, 55));
    odd.step_minute(1);
    assert_eq!(time(&odd), (9, 0));
}

#[test]
fn a_week_is_named_from_its_first_day() {
    let names = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    assert_eq!(week_in_order(&names, MONDAY), names);
    assert_eq!(week_in_order(&names, SUNDAY), ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"]);
    assert_eq!(week_in_order(&names, 6), ["Sa", "Su", "Mo", "Tu", "We", "Th", "Fr"]);
}
