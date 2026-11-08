//! A calendar and a clock for anything that needs a date, in a sheet over the window that asks.

use crate::{DayCell, MomentWindow, Say, look, modal};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use proton_core::{
    calendar::{DAY_FORMAT, Picking, week_in_order},
    timing::{self, FIELD_FORMAT},
};
use slint::{ComponentHandle, LogicalSize, ModelRc, Timer, VecModel, Window};
use std::{cell::RefCell, rc::Rc, time::Duration};

const WIDTH: f32 = 344.0;
const HEIGHT_WITH_TIME: f32 = 584.0;
const HEIGHT_DATE_ONLY: f32 = 460.0;
const NOON: u32 = 12;
/// A search field takes this too, as a person would write it.
const DOTTED_DAY: &str = "%d.%m.%Y";

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

pub struct Request {
    pub heading: String,
    /// Where the calendar opens: what the field holds now, or a sensible first guess.
    pub value: String,
    pub with_time: bool,
    /// Days before this one cannot be chosen.
    pub earliest: Option<NaiveDate>,
    /// Says what is wrong with a choice, as a key of the dictionary.
    pub check: Option<fn(&str) -> Option<&'static str>>,
}

struct Open {
    window: MomentWindow,
    picking: Picking,
    cells: Vec<NaiveDate>,
    with_time: bool,
    earliest: Option<NaiveDate>,
    check: Option<fn(&str) -> Option<&'static str>>,
    chosen: Option<Box<dyn FnOnce(String)>>,
    let_go: Box<dyn Fn(&Window)>,
}

fn field_of(moment: NaiveDateTime, with_time: bool) -> String {
    moment.format(if with_time { FIELD_FORMAT } else { DAY_FORMAT }).to_string()
}

fn moment_of(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, FIELD_FORMAT)
        .ok()
        .or_else(|| {
            let day = NaiveDate::parse_from_str(value, DAY_FORMAT).or_else(|_| NaiveDate::parse_from_str(value, DOTTED_DAY));
            day.ok()?.and_hms_opt(0, 0, 0)
        })
}

fn hour_shown(hour: u32, twelve: bool) -> (String, String) {
    if !twelve {
        return (format!("{hour:02}"), String::new());
    }
    let shown = match hour % NOON {
        0 => NOON,
        other => other,
    };
    (format!("{shown:02}"), if hour < NOON { "AM" } else { "PM" }.to_string())
}

fn cell_of(day: NaiveDate, picking: &Picking, today: NaiveDate, earliest: Option<NaiveDate>) -> DayCell {
    DayCell {
        number: i32::try_from(day.day()).unwrap_or(0),
        inside: day.month() == picking.month.month(),
        today: day == today,
        picked: day == picking.moment.date(),
        enabled: earliest.is_none_or(|earliest| day >= earliest),
    }
}

fn draw() {
    let Some((window, picking, earliest, with_time)) =
        OPEN.with_borrow(|open| open.as_ref().map(|open| (open.window.clone_strong(), open.picking, open.earliest, open.with_time)))
    else {
        return;
    };
    let say = window.global::<Say>();
    let months = say.invoke_text("date.months".into());
    let name = months.split(',').nth(picking.month.month0() as usize).unwrap_or_default();
    window.set_month(format!("{name} {}", picking.month.year()).into());
    let first = timing::first_day_of_week();
    let cells = picking.cells(first);
    let today = timing::today();
    let shown: Vec<DayCell> = cells.iter().map(|day| cell_of(*day, &picking, today, earliest)).collect();
    window.set_days(ModelRc::from(Rc::new(VecModel::from(shown))));
    window.set_chosen(timing::wall_text(picking.moment, with_time).into());
    let (hour, part) = hour_shown(picking.moment.hour(), timing::uses_twelve_hours());
    window.set_hour(hour.into());
    window.set_minute(format!("{:02}", picking.moment.minute()).into());
    window.set_part_of_day(part.into());
    window.set_problem("".into());
    OPEN.with_borrow_mut(|open| {
        if let Some(open) = open {
            open.cells = cells;
        }
    });
}

fn change(step: impl FnOnce(&mut Picking)) {
    OPEN.with_borrow_mut(|open| {
        if let Some(open) = open {
            step(&mut open.picking);
        }
    });
    draw();
}

fn close() {
    let Some(open) = OPEN.take() else { return };
    (open.let_go)(open.window.window());
    let _ = open.window.hide();
    Timer::single_shot(Duration::ZERO, move || drop(open));
}

fn confirmed() {
    let Some((window, moment, with_time, earliest, check)) = OPEN.with_borrow(|open| {
        open.as_ref().map(|open| (open.window.clone_strong(), open.picking.moment, open.with_time, open.earliest, open.check))
    }) else {
        return;
    };
    if earliest.is_some_and(|earliest| moment.date() < earliest) {
        return;
    }
    let field = field_of(moment, with_time);
    if let Some(problem) = check.and_then(|check| check(&field)) {
        window.set_problem(window.global::<Say>().invoke_text(problem.into()));
        return;
    }
    let chosen = OPEN.with_borrow_mut(|open| open.as_mut().and_then(|open| open.chosen.take()));
    close();
    if let Some(chosen) = chosen {
        chosen(field);
    }
}

fn weekdays(window: &MomentWindow) -> ModelRc<slint::SharedString> {
    let names = window.global::<Say>().invoke_text("date.weekdays".into());
    let names: Vec<&str> = names.split(',').collect();
    let ordered = week_in_order(&names, timing::first_day_of_week());
    ModelRc::from(Rc::new(VecModel::from(ordered.into_iter().map(slint::SharedString::from).collect::<Vec<_>>())))
}

/// The parent stays out of reach until this closes, and `chosen` gets what the field should hold.
pub fn open<C: ComponentHandle + 'static>(parent: &C, request: &Request, chosen: impl FnOnce(String) + 'static) {
    if OPEN.with_borrow(Option::is_some) {
        return;
    }
    let Ok(window) = MomentWindow::new() else { return };
    look::dress(&window.global::<crate::Theme>());
    window.set_heading(request.heading.as_str().into());
    window.set_with_time(request.with_time);
    window.set_own_close(cfg!(target_os = "macos"));
    window.set_weekdays(weekdays(&window));
    window.on_dismissed(close);
    window.on_confirmed(confirmed);
    window.on_step_month(|delta| change(|picking| picking.step_month(delta)));
    window.on_step_day(|delta| {
        let earliest = OPEN.with_borrow(|open| open.as_ref().and_then(|open| open.earliest));
        change(|picking| step_within(picking, delta, earliest));
    });
    window.on_step_hour(|delta| change(|picking| picking.step_hour(delta)));
    window.on_step_minute(|delta| change(|picking| picking.step_minute(delta)));
    window.on_pick(|index| {
        let day = OPEN.with_borrow(|open| open.as_ref().and_then(|open| open.cells.get(usize::try_from(index).ok()?).copied()));
        if let Some(day) = day {
            change(|picking| picking.pick(day));
        }
    });
    window.on_go_today(|| change(|picking| picking.pick(timing::today())));
    window.window().on_close_requested(|| {
        close();
        slint::CloseRequestResponse::KeepWindowShown
    });
    let height = if request.with_time { HEIGHT_WITH_TIME } else { HEIGHT_DATE_ONLY };
    window.window().set_size(LogicalSize::new(WIDTH, height));
    let moment = moment_of(&request.value).unwrap_or_else(start_of_today);
    let let_go = {
        let parent = parent.as_weak();
        Box::new(move |overlay: &Window| {
            if let Some(parent) = parent.upgrade() {
                modal::dismiss(parent.window(), overlay);
            }
        })
    };
    OPEN.set(Some(Open {
        window: window.clone_strong(),
        picking: Picking::new(moment),
        cells: Vec::new(),
        with_time: request.with_time,
        earliest: request.earliest,
        check: request.check,
        chosen: Some(Box::new(chosen)),
        let_go,
    }));
    draw();
    let _ = window.show();
    let parent = parent.as_weak();
    modal::after_show(&window, move |window| {
        if let Some(parent) = parent.upgrade() {
            modal::present(parent.window(), window.window());
        }
        window.invoke_begin();
    });
}

fn start_of_today() -> NaiveDateTime {
    timing::today().and_hms_opt(0, 0, 0).unwrap_or_default()
}

/// The arrow keys stop at the first day allowed, as a click does.
fn step_within(picking: &mut Picking, delta: i32, earliest: Option<NaiveDate>) {
    let before = *picking;
    picking.step_day(delta);
    if earliest.is_some_and(|earliest| picking.moment.date() < earliest) {
        *picking = before;
    }
}

#[cfg(test)]
mod tests {
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
}
