//! What to print and how. The system's own panel comes after this
//! one, and chooses the printer, the paper and the copies.

use crate::{
    App, PrintWindow, SettingsRow, actions, modal, preferences, print_view,
    settings::{
        rows::{field, segmented, switch},
        say,
    },
    view,
};
use proton_core::{
    PrintScope, Service,
    print::{FoldMarks, PrintDocument, PrintLabels, PrintOptions, test_sheet},
};
use slint::{ComponentHandle, Model, ModelRc, Timer, VecModel};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 680.0;
const SHIFT: &str = "shift";
const MAX_SHIFT_MM: f64 = 20.0;
const FOLDS: [FoldMarks; 3] = [FoldMarks::None, FoldMarks::A, FoldMarks::B];
const SWITCHES: [(&str, &str, &str); 10] = [
    ("headers", "print.headers", ""),
    ("technical", "print.technical", "print.technicalHint"),
    ("history", "print.history", "print.historyHint"),
    ("images", "print.images", ""),
    ("grayscale", "print.grayscale", ""),
    ("saveInk", "print.saveInk", "print.saveInkHint"),
    ("newSheetEach", "print.newSheet", ""),
    ("holeMark", "print.holeMark", ""),
    ("checkCode", "print.checkCode", "print.checkCodeHint"),
    ("seal", "print.seal", "print.sealHint"),
];

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

struct Open {
    window: PrintWindow,
    scopes: Vec<(PrintScope, String)>,
    scope: usize,
}

fn flag<'a>(options: &'a mut PrintOptions, key: &str) -> Option<&'a mut bool> {
    match key {
        "headers" => Some(&mut options.headers),
        "technical" => Some(&mut options.technical),
        "history" => Some(&mut options.history),
        "images" => Some(&mut options.images),
        "grayscale" => Some(&mut options.grayscale),
        "saveInk" => Some(&mut options.save_ink),
        "newSheetEach" => Some(&mut options.new_sheet_each),
        "holeMark" => Some(&mut options.hole_mark),
        "checkCode" => Some(&mut options.check_code),
        "seal" => Some(&mut options.seal),
        _ => None,
    }
}

fn scopes_for(app: &App) -> Vec<(PrintScope, String)> {
    let mut scopes = Vec::new();
    if view::current_id().is_some() {
        scopes.push((PrintScope::Message, say(app, "print.scopeMessage")));
        scopes.push((PrintScope::Thread, say(app, "print.scopeThread")));
    }
    let checked = app.get_checked_count();
    if checked > 1 {
        scopes.push((PrintScope::Selected, say(app, "print.scopeSelected").replace("{count}", &checked.to_string())));
        scopes.push((PrintScope::SelectedThreads, say(app, "print.scopeSelectedThreads")));
    }
    scopes
}

fn rows(app: &App, scopes: &[(PrintScope, String)], scope: usize) -> Vec<SettingsRow> {
    let mut options = preferences::get().print;
    let mut lines = Vec::new();
    if scopes.len() > 1 {
        lines.push(segmented(
            "scope",
            &say(app, "print.what"),
            scopes.iter().map(|(_, label)| label.clone()).collect(),
            scope,
        ));
    }
    for (key, label, hint) in SWITCHES {
        let hint = if hint.is_empty() { String::new() } else { say(app, hint) };
        let on = flag(&mut options, key).is_some_and(|on| *on);
        lines.push(switch(key, &say(app, label), &hint, on));
    }
    let folds = ["print.foldNone", "print.foldA", "print.foldB"].map(|key| say(app, key)).to_vec();
    let chosen = FOLDS.iter().position(|fold| *fold == options.fold_marks).unwrap_or(0);
    lines.push(segmented("folds", &say(app, "print.foldMarks"), folds, chosen));
    if options.fold_marks != FoldMarks::None || options.hole_mark {
        lines.push(field(SHIFT, &say(app, "print.shift"), &options.shift_mm.to_string()));
    }
    lines.push(crate::settings::rows::note(&say(app, "print.systemHint")));
    lines
}

fn draw(app: &App) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            open.window.set_rows(ModelRc::from(Rc::new(VecModel::from(rows(app, &open.scopes, open.scope)))));
        }
    });
}

fn keep_shift() {
    OPEN.with_borrow(|open| {
        let Some(open) = open else { return };
        let row = open.window.get_rows().iter().find(|row| row.id == SHIFT);
        if let Some(row) = row {
            let shift = row.text.trim().replace(',', ".").parse().unwrap_or(0.0_f64);
            preferences::change(|preferences| preferences.print.shift_mm = shift.clamp(-MAX_SHIFT_MM, MAX_SHIFT_MM));
        }
    });
}

pub fn labels(app: &App) -> PrintLabels {
    let say = |key: &str| say(app, key);
    PrintLabels {
        from: say("details.from"),
        to: say("details.to"),
        cc: say("details.cc"),
        date: say("details.date"),
        date_format: say("print.dateFormat"),
        attachments: say("print.attachments"),
        encryption: say("print.encryption"),
        size: say("print.size"),
        message_id: say("print.messageId"),
        first_mail: say("print.firstMail"),
        earlier_mail: say("print.earlierMail"),
        test_title: say("print.testTitle"),
        test_text: say("print.testText"),
        seal_title: say("print.sealTitle"),
        seal_text: say("print.sealText"),
        seal_key: say("print.sealKey"),
        seal_fingerprint: say("print.sealFingerprint"),
        seal_more: say("print.sealMore"),
    }
}

/// The sheet leaves first: the system's panel is a sheet on the same window.
fn hand_over(prepared: PrintDocument) {
    close();
    if let Some(app) = actions::main_window() {
        print_view::print(&app, prepared);
    }
}

fn prepare(service: &Arc<Service>, app: &App) {
    keep_shift();
    let scope =
        OPEN.with_borrow(|open| open.as_ref().and_then(|open| open.scopes.get(open.scope).map(|(scope, _)| *scope)));
    let Some(scope) = scope else { return };
    let (service, options, labels) = (service.clone(), preferences::get().print, labels(app));
    set_busy(true);
    tokio::spawn(async move {
        let prepared = service.print_document(scope, &options, &labels).await;
        let _ = slint::invoke_from_event_loop(move || match prepared {
            Ok(prepared) => hand_over(prepared),
            Err(error) => {
                log::warn!("a document could not be prepared: {}", error.kind());
                set_busy(false);
                show_problem(&error);
            }
        });
    });
}

fn set_busy(busy: bool) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            open.window.set_busy(busy);
        }
    });
}

fn show_problem(error: &proton_core::Error) {
    let Some(app) = actions::main_window() else { return };
    let text = match error {
        proton_core::Error::TooManyToPrint { limit } => {
            say(&app, "print.tooMany").replace("{limit}", &limit.to_string())
        }
        proton_core::Error::TooLongToSeal => say(&app, "print.tooLongToSeal"),
        other => other.to_string(),
    };
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            open.window.set_problem(text.into());
        }
    });
}

fn heard(app: &App, id: &str, value: i32) {
    let position = usize::try_from(value).unwrap_or(0);
    match id {
        "scope" => {
            OPEN.with_borrow_mut(|open| {
                if let Some(open) = open {
                    open.scope = position;
                }
            });
            return;
        }
        "folds" => preferences::change(|preferences| {
            preferences.print.fold_marks = FOLDS.get(position).copied().unwrap_or(FoldMarks::None);
        }),
        key => preferences::change(|preferences| {
            if let Some(on) = flag(&mut preferences.print, key) {
                *on = value == 1;
            }
        }),
    }
    draw(app);
}

fn close() {
    let Some(open) = OPEN.take() else { return };
    if let Some(app) = actions::main_window() {
        modal::dismiss(app.window(), open.window.window());
    }
    let _ = open.window.hide();
    Timer::single_shot(Duration::ZERO, move || drop(open));
}

pub fn open(service: &Arc<Service>, app: &App) {
    if OPEN.with_borrow(Option::is_some) {
        return;
    }
    let scopes = scopes_for(app);
    if scopes.is_empty() {
        return;
    }
    let Ok(window) = PrintWindow::new() else { return };
    crate::look::dress(&window.global::<crate::Theme>());
    window.set_own_close(cfg!(target_os = "macos"));
    window.on_dismissed(close);
    let weak = app.as_weak();
    window.on_event(move |id, _, value| {
        if let Some(app) = weak.upgrade() {
            heard(&app, &id, value);
        }
    });
    window.on_edited(|_| keep_shift());
    let weak = app.as_weak();
    window.on_test_sheet(move || {
        keep_shift();
        if let Some(app) = weak.upgrade() {
            hand_over(test_sheet(&preferences::get().print, &labels(&app)));
        }
    });
    let (sending, weak) = (service.clone(), app.as_weak());
    window.on_send(move || {
        if let Some(app) = weak.upgrade() {
            prepare(&sending, &app);
        }
    });
    window.window().on_close_requested(|| {
        close();
        slint::CloseRequestResponse::KeepWindowShown
    });
    window.set_rows(ModelRc::from(Rc::new(VecModel::from(rows(app, &scopes, 0)))));
    window.window().set_size(modal::fitted(app, WIDTH, HEIGHT));
    let _ = window.show();
    let parent = app.as_weak();
    modal::after_show(&window, move |window| {
        if let Some(app) = parent.upgrade() {
            modal::present(app.window(), window.window());
        }
        window.invoke_begin();
    });
    // Several chosen in the list come before the open mail, as they do on the page.
    let scope = scopes.iter().position(|(scope, _)| *scope == PrintScope::Selected).unwrap_or(0);
    OPEN.set(Some(Open { window, scopes, scope }));
    if scope != 0 {
        draw(app);
    }
}

/// The window the keyboard is with, when it is this panel.
#[cfg(target_os = "macos")]
pub fn with_window<F: FnOnce(&slint::Window)>(then: F) -> Option<F> {
    let window = OPEN.with_borrow(|open| open.as_ref().map(|open| ComponentHandle::clone_strong(&open.window)));
    match window {
        Some(window) => {
            then(window.window());
            None
        }
        None => Some(then),
    }
}
