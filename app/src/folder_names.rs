//! Adding, renaming and removing folders and labels from the sidebar, the name asked for in a dialog.

use crate::{App, NameWindow, actions, auth, look, modal, settings::{self, Ctx}};
use proton_core::{Service, model::Folder};
use slint::{ComponentHandle, Timer, Weak};
use std::{cell::RefCell, future::Future, sync::Arc, time::Duration};

const WIDTH: f32 = 400.0;
const HEIGHT: f32 = 176.0;
const LABELS: i32 = 2;
const SWITCH_TO: &str = "inbox";

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

#[derive(Clone)]
enum Naming {
    New(&'static str),
    Rename { id: String, color: String },
}

struct Open {
    window: NameWindow,
    naming: Naming,
}

fn close() {
    let Some(open) = OPEN.take() else { return };
    if let Some(app) = actions::main_window() {
        modal::dismiss(app.window(), open.window.window());
    }
    let _ = open.window.hide();
    Timer::single_shot(Duration::ZERO, move || drop(open));
}

fn window() -> Option<NameWindow> {
    OPEN.with_borrow(|open| open.as_ref().map(|open| open.window.clone_strong()))
}

fn finish(service: &Arc<Service>, app: &Weak<App>, answer: proton_core::error::Result<()>) {
    let (service, app, ctx) = (service.clone(), app.clone(), Ctx { service: service.clone(), app: app.clone() });
    let weak = app.clone();
    let _ = app.upgrade_in_event_loop(move |_| {
        match answer {
            Ok(()) => {
                close();
                auth::refresh_folders(&service, &weak);
            }
            Err(error) => {
                if let Some(window) = window() {
                    window.set_busy(false);
                    window.set_problem(ctx.error_text(&error).into());
                }
            }
        }
    });
}

fn confirmed(service: &Arc<Service>, app: &Weak<App>) {
    let Some(window) = window() else { return };
    let name = window.get_text().trim().to_string();
    let Some(naming) = OPEN.with_borrow(|open| open.as_ref().map(|open| open.naming.clone())) else { return };
    if name.is_empty() || window.get_busy() {
        return;
    }
    window.set_busy(true);
    window.set_problem("".into());
    let (service, app) = (service.clone(), app.clone());
    tokio::spawn(async move {
        let answer = match naming {
            Naming::New(kind) => service.create_label(&name, kind).await,
            Naming::Rename { id, color } => service.rename_label(&id, &name, &color).await,
        };
        finish(&service, &app, answer);
    });
}

fn ask(service: &Arc<Service>, app: &App, naming: Naming, heading: &str, action: &str, name: &str) {
    if OPEN.with_borrow(Option::is_some) {
        return;
    }
    let Ok(window) = NameWindow::new() else { return };
    look::dress(&window.global::<crate::Theme>());
    window.set_heading(heading.into());
    window.set_action(action.into());
    window.set_text(name.into());
    window.set_own_close(cfg!(target_os = "macos"));
    window.on_dismissed(close);
    let (naming_service, weak) = (service.clone(), app.as_weak());
    window.on_confirmed(move || confirmed(&naming_service, &weak));
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
    });
    OPEN.set(Some(Open { window, naming }));
}

fn add(service: &Arc<Service>, app: &App, section: i32) {
    let (kind, heading) = if section == LABELS { ("label", "nav.addLabel") } else { ("folder", "nav.addFolder") };
    ask(service, app, Naming::New(kind), &settings::say(app, heading), &settings::say(app, "common.create"), "");
}

fn with_folder(service: &Arc<Service>, app: &App, id: String, then: impl FnOnce(&App, Folder) + Send + 'static) {
    let (service, weak) = (service.clone(), app.as_weak());
    tokio::spawn(async move {
        let Ok(folders) = service.folders().await else {
            actions::toast(crate::toasts::FAILED);
            return;
        };
        if let Some(folder) = folders.into_iter().find(|folder| folder.id == id) {
            let _ = weak.upgrade_in_event_loop(move |app| then(&app, folder));
        }
    });
}

fn rename(service: &Arc<Service>, app: &App, id: String) {
    let asking = service.clone();
    with_folder(service, app, id, move |app, folder| {
        let naming = Naming::Rename { id: folder.id, color: folder.color.unwrap_or_default() };
        ask(&asking, app, naming, &settings::say(app, "nav.rename"), &settings::say(app, "common.save"), &folder.name);
    });
}

async fn removed(service: Arc<Service>, id: String, app: Weak<App>) {
    let answer = service.delete_label(&id).await;
    if answer.is_err() {
        actions::toast(crate::toasts::FAILED);
        return;
    }
    auth::refresh_folders_after(&service, &app, move |app| {
        // The mail in it stays, but the list of a folder that is gone cannot.
        if app.get_folder_id() == id.as_str() {
            app.invoke_open_folder(SWITCH_TO.into());
        }
    });
}

fn remove(service: &Arc<Service>, app: &App, id: String) {
    let removing = service.clone();
    with_folder(service, app, id, move |app, folder| {
        let title = settings::say(app, "nav.deleteTitle").replace("{name}", &folder.name);
        let (yes, no) = (settings::say(app, "nav.delete"), settings::say(app, "common.cancel"));
        if actions::confirmed(&title, &settings::say(app, "nav.deleteText"), &yes, &no) {
            spawn(removed(removing, folder.id, app.as_weak()));
        }
    });
}

fn spawn(task: impl Future<Output = ()> + Send + 'static) {
    tokio::spawn(task);
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let (adding, weak) = (service.clone(), app.as_weak());
    app.on_add_folder(move |section| {
        if let Some(app) = weak.upgrade() {
            add(&adding, &app, section);
        }
    });
    let (renaming, weak) = (service.clone(), app.as_weak());
    app.on_rename_folder(move |id| {
        if let Some(app) = weak.upgrade() {
            rename(&renaming, &app, id.to_string());
        }
    });
    let (removing, weak) = (service.clone(), app.as_weak());
    app.on_remove_folder(move |id| {
        if let Some(app) = weak.upgrade() {
            remove(&removing, &app, id.to_string());
        }
    });
}
