//! The search field and its builder. The language is the core's: this draws what it says about the
//! text, and asks for a search only once the text makes sense and the person has paused.

use crate::{App, SearchBuilder, SearchHint, SearchSpan, actions, modal, moment_picker, view};
use proton_core::{
    Service,
    search::{self, Form, Part, Problem},
};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel, Weak};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

const PAUSE: Duration = Duration::from_millis(300);
const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 560.0;

thread_local! {
    static WAITING: Timer = Timer::default();
    static BUILDER: RefCell<Option<SearchBuilder>> = const { RefCell::new(None) };
}

const fn part_of(part: Part) -> i32 {
    match part {
        Part::Space | Part::Word => 0,
        Part::Key => 1,
        Part::Value => 2,
        Part::Pattern => 3,
        Part::Negation => 4,
        Part::Wrong => 5,
    }
}

/// A code for the window to word, and the piece of text it is about.
fn problem_of(problem: Option<&Problem>) -> (i32, String) {
    match problem {
        None => (0, String::new()),
        Some(Problem::OpenQuote) => (1, String::new()),
        Some(Problem::OpenPattern) => (2, String::new()),
        Some(Problem::BadPattern) => (3, String::new()),
        Some(Problem::LongPattern) => (4, String::new()),
        Some(Problem::MissingValue(key)) => (5, format!("{key}:")),
        Some(Problem::BadDate(text)) => (6, text.clone()),
        Some(Problem::UnknownValue(text)) => (7, text.clone()),
        Some(Problem::DidYouMean(key)) => (8, format!("{key}:")),
        Some(Problem::WrongOrder) => (9, String::new()),
    }
}

fn show(app: &App, text: &str) -> bool {
    let parsed = search::parse(text);
    // A plain space at the end of a text would not be measured, and the colours would drift.
    let spans: Vec<SearchSpan> = parsed
        .spans
        .iter()
        .map(|span| SearchSpan { text: span.text.replace(' ', "\u{a0}").into(), part: part_of(span.part) })
        .collect();
    let hints: Vec<SearchHint> = search::hints(text)
        .iter()
        .map(|hint| SearchHint { insert: hint.insert.into(), explain: hint.explain })
        .collect();
    let (problem, word) = problem_of(parsed.problem.as_ref());
    app.set_query_spans(ModelRc::from(Rc::new(VecModel::from(spans))));
    app.set_query_hints(ModelRc::from(Rc::new(VecModel::from(hints))));
    app.set_query_problem(problem);
    app.set_query_word(word.into());
    parsed.problem.is_none()
}

fn edited(service: &Arc<Service>, app: &App, text: &str) {
    let sound = show(app, text);
    WAITING.with(Timer::stop);
    if sound {
        let (service, weak, text) = (service.clone(), app.as_weak(), text.to_string());
        WAITING.with(|timer| {
            timer.start(TimerMode::SingleShot, PAUSE, move || {
                view::apply_filters(&service, &weak, Some(text.clone()));
            });
        });
    }
}

pub fn abandon() {
    close_builder();
    WAITING.with(Timer::stop);
}

fn close_builder() {
    let Some(window) = BUILDER.take() else { return };
    if let Some(app) = actions::main_window() {
        modal::dismiss(app.window(), window.window());
    }
    let _ = window.hide();
    Timer::single_shot(Duration::ZERO, move || drop(window));
}

fn form_of(window: &SearchBuilder) -> Form {
    Form {
        words: window.get_words().to_string(),
        from: window.get_from().to_string(),
        to: window.get_to().to_string(),
        subject: window.get_subject().to_string(),
        pattern: window.get_pattern().to_string(),
        after: window.get_after().to_string(),
        before: window.get_before().to_string(),
        has_attachment: window.get_has_attachment(),
        unread: window.get_unread(),
        starred: window.get_starred(),
    }
}

fn pick_date(window: &SearchBuilder, heading: String, from: bool) {
    let value = if from { window.get_after() } else { window.get_before() };
    let request = moment_picker::Request { heading, value: value.to_string(), with_time: false, earliest: None, check: None };
    let weak = window.as_weak();
    moment_picker::open(window, &request, move |field| {
        let Some(window) = weak.upgrade() else { return };
        if from {
            window.set_after(field.into());
        } else {
            window.set_before(field.into());
        }
        preview(&window);
    });
}

fn preview(window: &SearchBuilder) {
    let text = form_of(window).text();
    let (problem, word) = problem_of(search::parse(&text).problem.as_ref());
    window.set_preview(text.into());
    window.set_problem(problem);
    window.set_word(word.into());
}

fn open_builder(service: &Arc<Service>, app: &App) {
    if BUILDER.with_borrow(Option::is_some) {
        return;
    }
    let Ok(window) = SearchBuilder::new() else { return };
    crate::look::dress(&window.global::<crate::Theme>());
    let form = Form::read(&app.get_query_text());
    window.set_words(form.words.as_str().into());
    window.set_from(form.from.as_str().into());
    window.set_to(form.to.as_str().into());
    window.set_subject(form.subject.as_str().into());
    window.set_pattern(form.pattern.as_str().into());
    window.set_after(form.after.as_str().into());
    window.set_before(form.before.as_str().into());
    window.set_has_attachment(form.has_attachment);
    window.set_unread(form.unread);
    window.set_starred(form.starred);
    window.set_own_close(cfg!(target_os = "macos"));
    preview(&window);
    let weak = window.as_weak();
    window.on_changed(move || {
        if let Some(window) = weak.upgrade() {
            preview(&window);
        }
    });
    window.on_dismissed(close_builder);
    let weak = window.as_weak();
    window.on_pick_date(move |heading, which| {
        if let Some(window) = weak.upgrade() {
            pick_date(&window, heading.to_string(), which == 0);
        }
    });
    let (applying, main, weak) = (service.clone(), app.as_weak(), window.as_weak());
    window.on_apply(move || {
        let (Some(window), Some(app)) = (weak.upgrade(), main.upgrade()) else { return };
        let text = form_of(&window).text();
        if search::parse(&text).problem.is_some() {
            return;
        }
        app.set_query_text(text.as_str().into());
        app.invoke_caret_to_end();
        close_builder();
        edited(&applying, &app, &text);
    });
    window.window().on_close_requested(|| {
        close_builder();
        slint::CloseRequestResponse::KeepWindowShown
    });
    window.window().set_size(modal::fitted(app, WIDTH, HEIGHT));
    let _ = window.show();
    let parent = app.as_weak();
    modal::after_show(&window, move |window| {
        if let Some(app) = parent.upgrade() {
            modal::present(app.window(), window.window());
        }
        window.invoke_begin();
    });
    BUILDER.set(Some(window));
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let (typing, weak) = (service.clone(), app.as_weak());
    app.on_search(move |text| {
        if let Some(app) = weak.upgrade() {
            edited(&typing, &app, &text);
        }
    });
    let (choosing, weak) = (service.clone(), app.as_weak());
    app.on_hint_chosen(move |insert| {
        let Some(app) = weak.upgrade() else { return };
        let text = search::with_hint(&app.get_query_text(), &insert);
        app.set_query_text(text.as_str().into());
        app.invoke_caret_to_end();
        edited(&choosing, &app, &text);
    });
    let (building, weak): (Arc<Service>, Weak<App>) = (service.clone(), app.as_weak());
    app.on_builder_requested(move || {
        if let Some(app) = weak.upgrade() {
            open_builder(&building, &app);
        }
    });
    show(app, "");
}

/// The window the keyboard is with, when it is the builder.
#[cfg(target_os = "macos")]
pub fn with_window<F: FnOnce(&slint::Window)>(then: F) -> Option<F> {
    let window = BUILDER.with_borrow(|open| open.as_ref().map(ComponentHandle::clone_strong));
    match window {
        Some(window) => {
            then(window.window());
            None
        }
        None => Some(then),
    }
}
