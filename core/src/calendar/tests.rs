use super::*;

const MONDAY: u8 = 1;
const SUNDAY: u8 = 7;

fn picking(text: &str) -> Picking {
    Picking::new(NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap())
}

fn time(picking: &Picking) -> (u32, u32) {
    (picking.moment.hour(), picking.moment.minute())
}

#[test]
fn a_grid_has_six_weeks_and_starts_on_the_chosen_day() {
    let cells = picking("2026-11-15 09:00").cells(MONDAY);
    assert_eq!(cells.len(), 42);
    assert_eq!(cells[0], NaiveDate::from_ymd_opt(2026, 10, 26).unwrap());
    let cells = picking("2026-11-15 09:00").cells(SUNDAY);
    assert_eq!(cells[0], NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
    assert!(cells.iter().step_by(7).all(|day| day.weekday().number_from_monday() == 7));
}

#[test]
fn every_day_of_every_month_is_on_its_grid() {
    for month in 1..=12 {
        for first in 1..=7 {
            let shown = picking(&format!("2028-{month:02}-01 09:00"));
            let cells = shown.cells(first);
            let inside = cells.iter().filter(|day| day.month() == month).count();
            let next = shown.month.checked_add_months(Months::new(1)).unwrap();
            assert_eq!(i64::try_from(inside).unwrap(), (next - shown.month).num_days());
        }
    }
}

#[test]
fn turning_a_month_leaves_the_choice_alone() {
    let mut picking = picking("2027-01-31 09:00");
    let chosen = picking.moment;
    picking.step_month(-1);
    assert_eq!(picking.month, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
    picking.step_month(2);
    assert_eq!(picking.month, NaiveDate::from_ymd_opt(2027, 2, 1).unwrap());
    assert_eq!(picking.moment, chosen);
}

#[test]
fn choosing_a_day_keeps_the_time_and_shows_its_month() {
    let mut picking = picking("2026-11-30 14:25");
    picking.pick(NaiveDate::from_ymd_opt(2026, 12, 2).unwrap());
    assert_eq!(picking.moment.date(), NaiveDate::from_ymd_opt(2026, 12, 2).unwrap());
    assert_eq!(time(&picking), (14, 25));
    assert_eq!(picking.month, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
}

#[test]
fn stepping_days_crosses_a_month() {
    let mut picking = picking("2026-11-30 09:00");
    picking.step_day(1);
    assert_eq!(picking.moment.date(), NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
    picking.step_day(-7);
    assert_eq!(picking.moment.date(), NaiveDate::from_ymd_opt(2026, 11, 24).unwrap());
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
