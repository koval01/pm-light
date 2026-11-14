//! The command palette and the keyboard help. A command is what a button or a key already does, so
//! nothing here decides anything of its own.

use crate::{
    App, CommandRow, PaletteWindow, Words, actions, auth, contacts, look, modal, preferences, print_dialog, settings,
    view,
};
use proton_core::{
    Service,
    keymap::{self, Action, Shortcut},
    ranking,
};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

const WIDTH: f32 = 560.0;
const HEIGHT: f32 = 420.0;

#[derive(Clone)]
enum Run {
    Compose(&'static str),
    Save(i32),
    Open(String),
    Images,
    Info(i32),
    Print,
    Move(String),
    Label(String, bool),
    Go(String),
    OnlyUnread,
    OnlyFiles,
    Contacts,
    Help,
    SignOut,
    Settings(&'static str),
    Theme(u8),
    Language(&'static str),
}

#[derive(Clone)]
struct Command {
    label: String,
    hint: &'static str,
    run: Run,
}

fn command(label: impl Into<String>, hint: &'static str, run: Run) -> Command {
    Command { label: label.into(), hint, run }
}

struct Open {
    window: PaletteWindow,
    all: Vec<Command>,
    shown: Vec<usize>,
}

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

fn message_commands(app: &App, words: &Words<'_>) -> Vec<Command> {
    let mut list = Vec::new();
    if !app.get_has_mail() {
        return list;
    }
    list.push(command(words.get_reply(), "r", Run::Compose("reply")));
    list.push(command(words.get_reply_all(), "a", Run::Compose("replyAll")));
    list.push(command(words.get_forward(), "f", Run::Compose("forward")));
    let files = app.get_files();
    for index in 0..files.row_count() {
        if let Some(file) = files.row_data(index) {
            list.push(command(words.invoke_download(file.name), "", Run::Save(i32::try_from(index).unwrap_or(0))));
        }
    }
    let thread = app.get_thread();
    for index in 0..thread.row_count() {
        if let Some(row) = thread.row_data(index).filter(|row| !row.current) {
            list.push(command(words.invoke_open_mail(row.sender), "", Run::Open(row.id.to_string())));
        }
    }
    list.push(command(words.get_images(), "i", Run::Images));
    list.push(command(words.get_details(), "", Run::Info(1)));
    list.push(command(words.get_source(), "", Run::Info(4)));
    list.push(command(words.get_print(), "", Run::Print));
    list.push(command(words.get_headers(), "h", Run::Info(3)));
    for (id, name) in view::move_targets() {
        let name = app.invoke_folder_name(id.as_str().into(), name.as_str().into());
        list.push(command(words.invoke_move_to_folder(name), "", Run::Move(id)));
    }
    let applied = view::current_labels();
    for (id, name) in view::labels() {
        let on = applied.contains(&id);
        list.push(command(words.invoke_label_on(name.as_str().into()), if on { "✓" } else { "" }, Run::Label(id, !on)));
    }
    list
}

fn commands(app: &App) -> Vec<Command> {
    let words = app.global::<Words>();
    let mut list = vec![command(words.get_compose(), "c", Run::Compose("new"))];
    list.extend(message_commands(app, &words));
    for (id, name) in view::all_folders() {
        let name = app.invoke_folder_name(id.as_str().into(), name.as_str().into());
        list.push(command(words.invoke_go_to(name), "", Run::Go(id)));
    }
    list.push(command(words.get_filter_unread(), "", Run::OnlyUnread));
    list.push(command(words.get_filter_files(), "", Run::OnlyFiles));
    list.push(command(words.get_contacts(), "g c", Run::Contacts));
    list.push(command(words.get_shortcuts(), "?", Run::Help));
    list.push(command(words.get_sign_out(), "", Run::SignOut));
    list.extend(settings_commands(app));
    list
}

fn settings_commands(app: &App) -> Vec<Command> {
    let say = |key: &str| settings::say(app, key);
    let title = say("nav.settings");
    let mut list = vec![command(title.clone(), "", Run::Settings(settings::FIRST_TAB))];
    list.extend(
        settings::all_tabs().map(|tab| command(format!("{title}: {}", say(tab.title)), "", Run::Settings(tab.id))),
    );
    list.push(command(say("palette.themeLight"), "", Run::Theme(1)));
    list.push(command(say("palette.themeDark"), "", Run::Theme(2)));
    list.push(command(say("palette.themeSystem"), "", Run::Theme(0)));
    for (code, key) in settings::LANGUAGES {
        list.push(command(say("palette.language").replace("{name}", &say(key)), "", Run::Language(code)));
    }
    list
}

fn run(service: &Arc<Service>, app: &App, what: Run) {
    match what {
        Run::Compose(mode) => app.invoke_compose(mode.into()),
        Run::Save(index) => app.invoke_save_file(index),
        Run::Open(id) => app.invoke_open_mail(id.into()),
        Run::Images => {
            if app.get_images_held() {
                app.invoke_show_images();
            }
        }
        Run::Info(mode) => view::open_info(mode),
        Run::Print => print_dialog::open(service, app),
        Run::Move(target) => actions::move_to(target),
        Run::Label(label, on) => actions::set_label(label, on),
        Run::Go(folder) => {
            app.invoke_pulse();
            app.invoke_open_folder(folder.into());
        }
        Run::OnlyUnread => {
            app.set_only_unread(!app.get_only_unread());
            app.invoke_filters_changed();
        }
        Run::OnlyFiles => {
            app.set_only_files(!app.get_only_files());
            app.invoke_filters_changed();
        }
        Run::Contacts => contacts::open(service, app),
        Run::Help => settings::open(service, app, "keyboard"),
        Run::Settings(tab) => settings::open(service, app, tab),
        Run::Theme(theme) => {
            preferences::change(|preferences| preferences.theme = theme);
            look::apply(app);
        }
        Run::Language(code) => settings::choose_language(app, code),
        Run::SignOut => auth::ask_to_sign_out(service, app),
    }
}

fn show(open: &mut Open) {
    let labels: Vec<String> = open.all.iter().map(|command| command.label.clone()).collect();
    open.shown = ranking::rank(&labels, &open.window.get_query());
    let rows: Vec<CommandRow> = open
        .shown
        .iter()
        .filter_map(|at| open.all.get(*at))
        .map(|command| CommandRow { label: command.label.as_str().into(), hint: command.hint.into() })
        .collect();
    open.window.set_commands(ModelRc::from(Rc::new(VecModel::from(rows))));
    open.window.set_chosen(0);
}

fn close() -> Option<()> {
    let open = OPEN.take()?;
    if let Some(app) = actions::main_window() {
        modal::dismiss(app.window(), open.window.window());
    }
    let _ = open.window.hide();
    let window = open.window;
    Timer::single_shot(Duration::ZERO, move || drop(window));
    Some(())
}

pub fn open(service: &Arc<Service>, app: &App) {
    if close().is_some() {
        return;
    }
    let Ok(window) = PaletteWindow::new() else { return };
    crate::look::dress(&window.global::<crate::Theme>());
    window.set_hint(app.global::<Words>().get_palette());
    window.on_dismissed(|| {
        close();
    });
    window.on_edited(|| {
        OPEN.with_borrow_mut(|open| {
            if let Some(open) = open {
                show(open);
            }
        });
    });
    let (running, parent) = (service.clone(), app.as_weak());
    window.on_run(move |index| {
        let what = OPEN.with_borrow(|open| {
            let open = open.as_ref()?;
            let at = open.shown.get(usize::try_from(index).ok()?)?;
            open.all.get(*at).map(|command| command.run.clone())
        });
        let Some(what) = what else { return };
        close();
        // Once the sheet has let go: a command may open a sheet of its own.
        let (service, parent) = (running.clone(), parent.clone());
        Timer::single_shot(Duration::from_millis(350), move || {
            if let Some(app) = parent.upgrade() {
                run(&service, &app, what);
            }
        });
    });
    window.window().on_close_requested(|| {
        close();
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
    let mut open = Open { window, all: commands(app), shown: Vec::new() };
    show(&mut open);
    OPEN.set(Some(open));
}

/// What a shortcut does, in the words of the window.
pub fn action_text(app: &App, action: Action) -> SharedString {
    let words = app.global::<Words>();
    match action {
        Action::Next => words.get_next(),
        Action::Previous => words.get_previous(),
        Action::Open => words.get_open(),
        Action::Close => words.get_close(),
        Action::ScrollDown => words.get_scroll_down(),
        Action::ScrollUp => words.get_scroll_up(),
        Action::Search => words.get_search(),
        Action::Compose => words.get_compose(),
        Action::Reply => words.get_reply(),
        Action::ReplyAll => words.get_reply_all(),
        Action::Forward => words.get_forward(),
        Action::Archive => words.get_archive(),
        Action::Trash => words.get_trash(),
        Action::Spam => words.get_spam(),
        Action::Star => words.get_star(),
        Action::Unread => words.get_unread(),
        Action::Move => words.get_move_to(),
        Action::Label => words.get_label(),
        Action::Images => words.get_show_images(),
        Action::ToggleCheck => words.get_toggle_check(),
        Action::CheckAll => words.get_check_all(),
        Action::Headers => words.get_headers(),
        Action::Undo => words.get_undo(),
        Action::Help => words.get_help(),
        Action::Contacts => words.get_contacts(),
        Action::Go(folder) => app.invoke_folder_name(folder.into(), SharedString::new()),
    }
}

pub fn shortcut_rows(app: &App) -> Vec<(bool, String, String)> {
    let command = if cfg!(target_os = "macos") { "⌘ K" } else { "Ctrl K" };
    let words = app.global::<Words>();
    let mut rows = vec![(false, words.get_palette().to_string(), command.to_string())];
    rows.extend(
        keymap::shortcuts()
            .into_iter()
            .map(|Shortcut { action, keys, chord }| (chord, action_text(app, action).to_string(), keys)),
    );
    rows
}

/// The session is gone, and what the palette listed with it.
pub fn abandon() {
    close();
}

/// The window the keyboard is with, when it is this panel.
#[cfg(target_os = "macos")]
pub fn with_window<F: FnOnce(&slint::Window)>(then: F) -> Option<F> {
    // A copy of the handle: what `then` sets off may need `OPEN` too, as typing does here.
    let window = OPEN.with_borrow(|open| open.as_ref().map(|open| open.window.clone_strong()));
    match window {
        Some(window) => {
            then(window.window());
            None
        }
        None => Some(then),
    }
}
