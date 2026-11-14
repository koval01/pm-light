//! What keys do in the main window. `proton_core::keymap` says which key means what, and this does
//! it, the same way the matching button does.

use crate::{App, Settings, contacts, mail_view, palette, settings, tray, view};
use proton_core::{
    Service,
    keymap::{Action, KeyReader},
};
use slint::{ComponentHandle, Weak};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// The account can turn shortcuts off, and it has the last word.
static ALLOWED: AtomicBool = AtomicBool::new(true);

thread_local! {
    static READER: RefCell<KeyReader> = RefCell::new(KeyReader::default());
}

const CANNOT_DELETE_HERE: [&str; 2] = ["trash", "spam"];

fn in_folder(app: &App, folder: &str) -> bool {
    app.get_folder_id() == folder
}

fn move_targets(app: &App, target: &str) {
    if !in_folder(app, target) {
        app.invoke_act(target.into());
    }
}

fn trash(app: &App) {
    let forever = CANNOT_DELETE_HERE.iter().any(|folder| in_folder(app, folder));
    app.invoke_act(if forever { "delete" } else { "trash" }.into());
}

pub fn scroll(app: &App, direction: i32) {
    if app.get_has_mail() {
        mail_view::scroll_page(direction);
    } else {
        app.set_list_scroll(direction);
        app.set_list_scrolls(app.get_list_scrolls() + 1);
    }
}

fn close_or_clear(service: &Arc<Service>, app: &App, weak: &Weak<App>) {
    if app.get_checked_count() > 0 {
        service.view_check_all(false);
    } else {
        view::close_mail(service, weak);
    }
}

fn compose_if_open(app: &App, mode: &str) {
    if app.get_has_mail() {
        app.invoke_compose(mode.into());
    }
}

fn run(service: &Arc<Service>, app: &App, weak: &Weak<App>, action: Action) {
    match action {
        Action::Next => view::step(service, weak, 1),
        Action::Previous => view::step(service, weak, -1),
        Action::Open => {
            if in_folder(app, "drafts") {
                compose_if_open(app, "draft");
            }
        }
        Action::Close => close_or_clear(service, app, weak),
        Action::ScrollDown => scroll(app, 1),
        Action::ScrollUp => scroll(app, -1),
        Action::Search => app.invoke_caret_to_end(),
        Action::Compose => app.invoke_compose("new".into()),
        Action::Reply => compose_if_open(app, "reply"),
        Action::ReplyAll => compose_if_open(app, "replyAll"),
        Action::Forward => compose_if_open(app, "forward"),
        Action::Archive => move_targets(app, "archive"),
        Action::Trash => trash(app),
        Action::Spam => move_targets(app, "spam"),
        Action::Star => app.invoke_act("star".into()),
        Action::Unread => app.invoke_act("unread".into()),
        Action::Move => tray::show_action_menu(app, "move"),
        Action::Label => tray::show_action_menu(app, "labels"),
        Action::Images => {
            if app.get_images_held() {
                app.invoke_show_images();
            }
        }
        Action::ToggleCheck => {
            if let Some(id) = view::current_id() {
                service.view_toggle(&id);
            }
        }
        Action::CheckAll => service.view_check_all(true),
        Action::Headers => view::open_info(3),
        Action::Go(folder) => {
            app.invoke_pulse();
            app.invoke_open_folder(folder.into());
        }
        Action::Help => settings::open(service, app, "keyboard"),
        Action::Contacts => contacts::open(service, app),
        Action::Undo => {
            if app.get_undo_seconds() > 0 {
                app.invoke_undo_send();
            }
        }
    }
}

pub fn allow(on: bool) {
    ALLOWED.store(on, Ordering::Relaxed);
}

/// Once there is a session to ask with.
/// Once there is a session to ask with: the shortcuts, and whether the trash and spam empty themselves, which their
/// lists say.
pub fn read_settings(service: &Arc<Service>, app: &slint::Weak<crate::App>) {
    let (asking, app) = (service.clone(), app.clone());
    tokio::spawn(async move {
        if let Ok(settings) = asking.mail_settings().await {
            ALLOWED.store(settings.shortcuts, Ordering::Relaxed);
            let auto_delete = settings.auto_delete_old;
            let _ = app.upgrade_in_event_loop(move |app| app.set_auto_delete(auto_delete));
        }
    });
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let (service, weak) = (service.clone(), app.as_weak());
    app.on_shortcut(move |key| {
        let Some(app) = weak.upgrade() else { return false };
        if key == "Command+K" {
            palette::open(&service, &app);
            return true;
        }
        // The mail is not on screen, and a key would act on a selection nobody can see.
        if !ALLOWED.load(Ordering::Relaxed) || app.global::<Settings>().get_open() {
            return false;
        }
        let action = READER.with_borrow_mut(|reader| reader.read(&key));
        if let Some(action) = action {
            run(&service, &app, &weak, action);
        }
        action.is_some()
    });
}
