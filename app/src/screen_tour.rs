//! After the account check drew its mail, the window goes through its pages and the ways it can look, one at a time,
//! with the test account's real mail, and CI photographs the whole screen at each (`.github/workflows/live.yml`).
//!
//! The two take turns through files beside the check's note. Once a step has settled, the app writes its name to
//! `<file>.step`; CI photographs the screen under that name and removes the file, and the app goes on. A step CI never
//! answers is left after a while, so a missing photographer cannot hold the check. `<file>.done` ends the tour.
//!
//! Nothing here is kept: the look, the language and the window's size change only on screen, a letter opened here is
//! put away unsaved, and the mail the tour opened is marked unread again if it was so before.

use crate::{App, Settings, Theme, composer, look, mail_view, settings, view};
use proton_core::{
    Service,
    model::{MessageChanges, MessageMeta},
};
use slint::{ComponentHandle, LogicalSize, Timer, Weak, winit_030::WinitWindowAccessor};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// Long enough for a folder, a mail or a settings page to arrive and be drawn.
const SETTLE: Duration = Duration::from_millis(2500);
/// How often the app looks whether CI took its picture.
const LOOK: Duration = Duration::from_millis(200);
/// Past this a step goes on without its picture.
const WAIT: Duration = Duration::from_secs(30);
/// The smallest window the app allows.
const SMALLEST: LogicalSize = LogicalSize { width: 900.0, height: 580.0 };
const FOLDERS: [&str; 9] = ["starred", "drafts", "scheduled", "snoozed", "sent", "archive", "spam", "trash", "all"];
const LETTERS: [&str; 3] = ["new", "reply", "forward"];
/// Each with the look it is shown in: `(name, choice, palette)`, 1 light and 2 dark.
const LOOKS: [(&str, i32, i32); 9] = [
    ("dark", 2, 0),
    ("dark-graphite", 2, 1),
    ("dark-green", 2, 2),
    ("light-graphite", 1, 1),
    ("light-green", 1, 2),
    ("light-purple", 1, 3),
    ("light-sand", 1, 4),
    ("high-contrast", 1, 5),
    ("light", 1, 0),
];
const ZOOMS: [(&str, f32); 3] = [("zoom-125", 1.25), ("zoom-150", 1.5), ("zoom-100", 1.0)];
const LANGUAGES: [&str; 2] = ["ru", "de"];

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Mail(usize),
    Folder(&'static str),
    Search(&'static str),
    Letter(&'static str),
    Settings(&'static str),
    Smallest,
    Maximised,
    FullScreen,
    Look(&'static str, i32, i32),
    Rows(bool),
    Compact(bool),
    Zoom(&'static str, f32),
    Language(&'static str),
}

impl Step {
    /// A fixed word for the step, from this file only: it names the picture and the journal's record.
    fn word(&self) -> String {
        match self {
            Self::Mail(index) => format!("mail-{}", index + 1),
            Self::Folder(folder) => format!("folder-{folder}"),
            Self::Search(_) => "search".into(),
            Self::Letter(mode) => format!("letter-{mode}"),
            Self::Settings(tab) => format!("settings-{}", tab.to_ascii_lowercase()),
            Self::Smallest => "window-smallest".into(),
            Self::Maximised => "window-maximised".into(),
            Self::FullScreen => "window-full-screen".into(),
            Self::Look(name, ..) | Self::Zoom(name, _) => (*name).into(),
            Self::Rows(on) => if *on { "reader-below" } else { "reader-beside" }.into(),
            Self::Compact(on) => if *on { "compact" } else { "roomy" }.into(),
            Self::Language(language) => format!("language-{language}"),
        }
    }
}

/// Every step, in order, for an inbox of `mails` rows: the first is open already.
fn steps(mails: usize) -> Vec<Step> {
    let mut steps = vec![Step::Mail(0)];
    steps.extend((1..mails).map(Step::Mail));
    // While a mail of the inbox is open: a reply and a forward take the open mail.
    steps.extend(LETTERS.into_iter().map(Step::Letter));
    steps.extend(FOLDERS.into_iter().map(Step::Folder));
    steps.push(Step::Search("sale"));
    steps.extend(settings::tabs::all().map(|tab| Step::Settings(tab.id)));
    steps.extend([Step::Smallest, Step::Maximised, Step::FullScreen]);
    steps.extend(LOOKS.into_iter().map(|(name, choice, palette)| Step::Look(name, choice, palette)));
    steps.extend([Step::Rows(true), Step::Compact(true), Step::Rows(false), Step::Compact(false)]);
    steps.extend(ZOOMS.into_iter().map(|(name, zoom)| Step::Zoom(name, zoom)));
    steps.extend(LANGUAGES.into_iter().map(Step::Language));
    steps
}

/// The file a step's name goes into, numbered so that the pictures sort in the tour's order.
fn step_text(index: usize, step: &Step) -> String {
    format!("{:02}-{}", index + 1, step.word())
}

struct Tour {
    service: Arc<Service>,
    app: Weak<App>,
    rows: Vec<MessageMeta>,
    steps: Vec<Step>,
    next: usize,
    note: PathBuf,
    waited: Duration,
    timer: Timer,
}

thread_local! {
    static SEEN: RefCell<Option<(Arc<Service>, Vec<MessageMeta>)>> = const { RefCell::new(None) };
    static TOUR: RefCell<Option<Tour>> = const { RefCell::new(None) };
    /// The zoom the tour set, over the system's scale.
    static ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

/// The inbox the account check read, for the tour that follows it.
pub fn remember(service: &Arc<Service>, rows: Vec<MessageMeta>) {
    SEEN.set(Some((service.clone(), rows)));
}

fn beside(note: &Path, ending: &str) -> PathBuf {
    let mut name = note.as_os_str().to_os_string();
    name.push(ending);
    PathBuf::from(name)
}

/// From the account check, once its note is written.
pub fn start(app: &App, note: PathBuf) {
    let Some((service, rows)) = SEEN.take() else { return };
    let steps = steps(rows.len());
    log::info!("the screen tour: {} steps", steps.len());
    let _ = std::fs::remove_file(beside(&note, ".step"));
    let _ = std::fs::remove_file(beside(&note, ".done"));
    let tour = Tour { service, app: app.as_weak(), rows, steps, next: 0, note, waited: Duration::ZERO, timer: Timer::default() };
    TOUR.set(Some(tour));
    // Whatever else the machine opened must not stand in front of the pictures.
    app.window().with_winit_window(slint::winit_030::winit::window::Window::focus_window);
    // The first step is the window as the check left it.
    after(SETTLE, settled);
}

fn after(delay: Duration, then: fn()) {
    TOUR.with_borrow(|tour| {
        if let Some(tour) = tour {
            tour.timer.start(slint::TimerMode::SingleShot, delay, then);
        }
    });
}

/// The step is on screen: CI is asked for its picture.
fn settled() {
    let asked = TOUR.with_borrow(|tour| {
        let tour = tour.as_ref()?;
        let step = tour.steps.get(tour.next)?;
        std::fs::write(beside(&tour.note, ".step"), step_text(tour.next, step)).ok()
    });
    if asked.is_some() {
        after(LOOK, taken);
    } else {
        end();
    }
}

/// Whether CI took the picture; once it has, or gave up, the next step.
fn taken() {
    let waiting = TOUR.with_borrow_mut(|tour| {
        let tour = tour.as_mut()?;
        if beside(&tour.note, ".step").exists() && tour.waited < WAIT {
            tour.waited += LOOK;
            return Some(true);
        }
        if tour.waited >= WAIT {
            log::warn!("the screen tour: no picture of step {}", tour.next + 1);
        }
        tour.waited = Duration::ZERO;
        tour.next += 1;
        Some(false)
    });
    match waiting {
        Some(true) => after(LOOK, taken),
        Some(false) => {
            go_to_next();
            after(SETTLE, settled);
        }
        None => end(),
    }
}

/// Leaves the step shown and puts the next one on screen.
fn go_to_next() {
    let next = TOUR.with_borrow(|tour| {
        let tour = tour.as_ref()?;
        let app = tour.app.upgrade()?;
        let leaving = tour.next.checked_sub(1).and_then(|index| tour.steps.get(index)).cloned();
        let coming = tour.steps.get(tour.next).cloned();
        Some((tour.service.clone(), app, tour.rows.clone(), leaving, coming))
    });
    let Some((service, app, rows, leaving, coming)) = next else { return };
    if let Some(leaving) = &leaving {
        leave(&app, leaving, coming.as_ref());
    }
    if let Some(coming) = coming {
        show(&service, &app, &rows, &coming);
    }
}

fn leave(app: &App, leaving: &Step, coming: Option<&Step>) {
    match leaving {
        Step::Letter(_) => composer::abandon(),
        Step::Search(_) => {
            app.set_query_text("".into());
            app.invoke_search("".into());
        }
        Step::Settings(_) if !matches!(coming, Some(Step::Settings(_))) => app.global::<Settings>().invoke_back(),
        _ => {}
    }
}

fn show(service: &Arc<Service>, app: &App, rows: &[MessageMeta], step: &Step) {
    let window = app.window();
    match step {
        Step::Mail(index) => {
            if let Some(row) = rows.get(*index) {
                view::open_mail(service, &app.as_weak(), row.id.clone());
            }
        }
        Step::Folder(folder) => app.invoke_open_folder((*folder).into()),
        Step::Search(text) => {
            app.invoke_open_folder("inbox".into());
            app.set_query_text((*text).into());
            app.invoke_search((*text).into());
        }
        Step::Letter(mode) => app.invoke_compose((*mode).into()),
        Step::Settings(tab) => settings::open(service, app, tab),
        Step::Smallest => {
            window.set_size(SMALLEST);
            if let Some(row) = rows.first() {
                view::open_mail(service, &app.as_weak(), row.id.clone());
            }
        }
        Step::Maximised => window.set_maximized(true),
        Step::FullScreen => window.set_fullscreen(true),
        Step::Look(_, choice, palette) => {
            if window.is_fullscreen() {
                window.set_fullscreen(false);
                window.set_maximized(false);
            }
            let theme = app.global::<Theme>();
            theme.set_choice(*choice);
            theme.set_palette(*palette);
        }
        Step::Rows(on) => app.global::<Theme>().set_rows(*on),
        Step::Compact(on) => app.global::<Theme>().set_compact(*on),
        Step::Zoom(_, zoom) => {
            let system = window.scale_factor() / ZOOM.get();
            crate::rescale::rescale(window, system * zoom);
            ZOOM.set(*zoom);
            mail_view::zoom_to(f64::from(*zoom));
            look::sharpen_logo(app);
        }
        Step::Language(language) => {
            proton_core::timing::set_language(language);
            let _ = slint::select_bundled_translation(language);
        }
    }
    log::info!("the screen tour: {}", step.word());
}

/// The tour is over: the mail it opened is unread again where it was, the language is the person's, and CI is told.
fn end() {
    let Some(tour) = TOUR.take() else { return };
    look::apply_language();
    let unread: Vec<String> = tour.rows.iter().filter(|row| row.unread && row.messages == 0).map(|row| row.id.clone()).collect();
    let (service, done) = (tour.service.clone(), beside(&tour.note, ".done"));
    log::info!("the screen tour ended after {} steps; {} mails to mark unread again", tour.next, unread.len());
    tokio::spawn(async move {
        if !unread.is_empty() {
            let changes = MessageChanges { unread: Some(true), ..MessageChanges::default() };
            if let Err(error) = service.patch(&unread, &changes).await {
                log::warn!("the screen tour could not mark the mail unread again: {}", error.kind());
            }
        }
        let _ = std::fs::write(done, "done\n");
    });
}

#[cfg(test)]
mod tests;
