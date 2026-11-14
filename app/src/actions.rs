//! What a person can do to the open message. Each goes through the same core calls as the app in
//! `main`, so the rules, such as what may move where, are not written twice.

use crate::{App, Auth, composer, preview, reader, tray, view};
use parking_lot::Mutex;
use proton_core::{
    Service,
    compose::ComposeMode,
    file_kind,
    model::{Attachment, MessageChanges},
    safety,
    timing::SnoozeChoice,
};
use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use slint::{ComponentHandle, Weak};
use std::{
    path::Path,
    sync::{Arc, OnceLock},
};

static SERVICE: OnceLock<Arc<Service>> = OnceLock::new();
static WINDOW: Mutex<Option<Weak<App>>> = Mutex::new(None);

use crate::toasts::{BLOCKED, FAILED, SAVED};

pub fn connect(service: &Arc<Service>, app: &App) {
    let _ = SERVICE.set(service.clone());
    *WINDOW.lock() = Some(app.as_weak());
    app.on_act(|action| run(action.as_str()));
    let toggling = service.clone();
    app.on_row_toggled(move |id| toggling.view_toggle(&id));
    let checking = service.clone();
    app.on_check_all(move |on| checking.view_check_all(on));
    let starring = service.clone();
    app.on_row_starred(move |id, on| {
        let service = starring.clone();
        let changes = MessageChanges { starred: Some(on), ..Default::default() };
        spawn("marking", async move { service.view_mark(Some(vec![id.to_string()]), changes).await });
    });
    let weak = app.as_weak();
    app.on_menu(move |kind| {
        if let Some(app) = weak.upgrade() {
            tray::show_action_menu(&app, kind.as_str());
        }
    });
    app.on_save_file(save_file);
    app.on_open_file(open_file);
    let (writing, weak) = (service.clone(), app.as_weak());
    app.on_compose(move |mode| {
        let Some(app) = weak.upgrade() else { return };
        let mode = match mode.as_str() {
            "reply" => ComposeMode::Reply,
            "replyAll" => ComposeMode::ReplyAll,
            "forward" => ComposeMode::Forward,
            "draft" => ComposeMode::Draft,
            _ => ComposeMode::New,
        };
        composer::start(&writing, &weak, mode, composer::labels(&app));
    });
}

pub fn after_compose(notice: i32) {
    let Some(service) = SERVICE.get().cloned() else { return };
    if notice != 0 {
        toast(notice);
    }
    tokio::spawn(async move { service.view_reload().await });
}

pub fn toast(code: i32) {
    let window = WINDOW.lock().clone();
    if let Some(app) = window {
        let _ = app.upgrade_in_event_loop(move |app| app.set_toast(code));
    }
}

/// The result of an action shows only when it went wrong: what it did is on screen already. `what`
/// is a fixed word for the journal, never anything taken from the mail.
fn spawn<F>(what: &'static str, work: F)
where
    F: Future<Output = proton_core::Result<()>> + Send + 'static,
{
    log::info!("{what}");
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        match work.await {
            Ok(()) => log::debug!("{what} done in {} ms", started.elapsed().as_millis()),
            Err(error) => {
                log::warn!("{what} failed: {}", error.kind());
                toast(FAILED);
            }
        }
    });
}

fn asked(level: MessageLevel, title: &str, text: &str, yes: &str, no: &str) -> bool {
    let window = main_window();
    let mut dialog = MessageDialog::new();
    // With a parent this is a sheet. Without one macOS shows it from another process, and the whole
    // program stops behind an alert that may not even be in front.
    if let Some(window) = &window {
        dialog = dialog.set_parent(&window.window().window_handle());
    }
    let answer = dialog
        .set_level(level)
        .set_title(title)
        .set_description(text)
        .set_buttons(MessageButtons::OkCancelCustom(yes.to_string(), no.to_string()))
        .show();
    match answer {
        MessageDialogResult::Custom(chosen) => chosen == yes,
        MessageDialogResult::Ok => true,
        _ => false,
    }
}

/// A question before something that cannot be taken back.
pub fn confirmed(title: &str, text: &str, yes: &str, no: &str) -> bool {
    asked(MessageLevel::Warning, title, text, yes, no)
}

pub fn ask_to_sign_out(app: &App) -> bool {
    let auth = app.global::<Auth>();
    asked(
        MessageLevel::Warning,
        &auth.get_word_sign_out_title(),
        &auth.get_word_sign_out_text(),
        &auth.get_word_sign_out(),
        &app.get_word_cancel(),
    )
}

pub fn ask_to_forget(app: &App) -> bool {
    let auth = app.global::<Auth>();
    asked(
        MessageLevel::Warning,
        &auth.get_word_forgot_title(),
        &auth.get_word_forgot_text(),
        &auth.get_word_sign_out(),
        &app.get_word_cancel(),
    )
}

/// Leaving Tor shows the person's address: asked with what it means, and the safe answer as the way out.
pub fn ask_to_leave_tor(app: &App) -> bool {
    let auth = app.global::<Auth>();
    asked(
        MessageLevel::Warning,
        &auth.get_word_leave_tor_title(),
        &auth.get_word_leave_tor_text(),
        &auth.get_word_leave_tor(),
        &app.get_word_cancel(),
    )
}

pub fn ask_to_unsubscribe(app: &App, text: &str) -> bool {
    asked(
        MessageLevel::Info,
        &app.get_word_unsubscribe_title(),
        text,
        &app.get_word_unsubscribe(),
        &app.get_word_cancel(),
    )
}

/// Everything here acts on what the core calls the targets: the rows ticked, or else the open one.
fn run(action: &str) {
    let Some(service) = SERVICE.get().cloned() else { return };
    let id = view::current_id().unwrap_or_default();
    let ids = None;
    match action {
        "unsubscribe" | "receipt" => {
            let Some(app) = main_window() else { return };
            if action == "receipt" {
                reader::send_receipt(service, &app);
            } else {
                reader::unsubscribe(service, &app);
            }
        }
        "unread" => {
            let changes = MessageChanges { unread: Some(true), ..Default::default() };
            spawn("marking", async move { service.view_mark(ids, changes).await });
        }
        "star" => {
            let on = WINDOW.lock().clone().and_then(|app| app.upgrade()).is_some_and(|app| !app.get_starred());
            let changes = MessageChanges { starred: Some(on), ..Default::default() };
            spawn("marking", async move { service.view_mark(ids, changes).await });
        }
        "spam" if unsubscribes_with_spam(&service, &id) => spawn("moving to spam", async move {
            // The move matters more: a refused request only means the mail keeps coming.
            if let Err(error) = service.unsubscribe(&id).await {
                log::warn!("unsubscribing on the way to spam failed: {}", error.kind());
            }
            service.view_move(ids, "spam").await
        }),
        "archive" | "spam" | "trash" => {
            let target = action.to_string();
            spawn("moving", async move { service.view_move(ids, &target).await });
        }
        "not-spam" => spawn("marking as not spam", async move { service.view_not_spam(&id).await }),
        "unsnooze" => spawn("unsnoozing", async move { service.view_unsnooze(&id).await }),
        "delete" => delete(service, ids),
        "empty" => empty_folder(service),
        _ => {}
    }
}

/// Only what Proton can do by itself: no other way of leaving a list happens without being asked.
fn unsubscribes_with_spam(service: &Service, id: &str) -> bool {
    let one_click = || {
        view::open_message().is_some_and(|message| {
            let open = message.meta.id == id || message.meta.conversation_id == id;
            open && message.unsubscribe.as_ref().is_some_and(|methods| methods.one_click)
        })
    };
    service.habits().unsubscribe_on_spam && one_click()
}

fn empty_folder(service: Arc<Service>) {
    let Some(app) = main_window() else { return };
    let yes = asked(
        MessageLevel::Warning,
        &app.get_word_empty_title(),
        &app.get_word_empty_text(),
        &app.get_word_empty(),
        &app.get_word_cancel(),
    );
    if yes {
        spawn("emptying a folder", async move { service.view_empty().await });
    }
}

fn delete(service: Arc<Service>, ids: Option<Vec<String>>) {
    let Some(app) = WINDOW.lock().clone().and_then(|app| app.upgrade()) else { return };
    let yes = asked(
        MessageLevel::Warning,
        &app.get_word_delete_title(),
        &app.get_word_delete_text(),
        &app.get_word_delete(),
        &app.get_word_cancel(),
    );
    if yes {
        spawn("deleting", async move { service.view_delete(ids).await });
    }
}

pub fn service() -> Option<Arc<Service>> {
    SERVICE.get().cloned()
}

pub fn main_window() -> Option<App> {
    WINDOW.lock().clone().and_then(|app| app.upgrade())
}

pub fn choose_sender(email: &str) {
    if let Some(service) = SERVICE.get() {
        composer::set_from(service, email);
    }
}

pub fn block_sender() {
    let (Some(service), Some(app)) = (SERVICE.get().cloned(), WINDOW.lock().clone().and_then(|app| app.upgrade()))
    else {
        return;
    };
    let Some(email) = view::current_sender() else { return };
    let title = app.invoke_block_title(email.as_str().into());
    let yes = service.habits().skip_block_question
        || asked(
            MessageLevel::Warning,
            &title,
            &app.get_word_block_text(),
            &app.get_word_block(),
            &app.get_word_cancel(),
        );
    if yes {
        tokio::spawn(async move {
            toast(if service.block_sender(&email).await.is_ok() { BLOCKED } else { FAILED });
        });
    }
}

pub fn snooze(choice: SnoozeChoice) {
    let (Some(service), Some(id)) = (SERVICE.get().cloned(), view::current_id()) else { return };
    spawn("snoozing", async move { service.view_snooze(&id, choice).await });
}

pub fn move_to(target: String) {
    let Some(service) = SERVICE.get().cloned() else { return };
    spawn("moving", async move { service.view_move(None, &target).await });
}

pub fn set_label(label: String, on: bool) {
    let Some(service) = SERVICE.get().cloned() else { return };
    spawn("labelling", async move { service.view_label(None, &label, on).await });
}

/// Tells the system this came from outside, so that it checks the file when it is first opened,
/// as it does for a browser download. A failure here must not lose the saved file.
#[cfg(target_os = "macos")]
fn mark_as_downloaded(path: &Path) {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let marked = xattr::set(path, "com.apple.quarantine", format!("0081;{seconds:x};Proton Mail Light;").as_bytes());
    if let Err(error) = marked {
        log::warn!("a saved file was not marked as downloaded: {:?}", error.kind());
    }
}

#[cfg(windows)]
fn mark_as_downloaded(path: &Path) {
    let mut mark = path.as_os_str().to_owned();
    mark.push(":Zone.Identifier");
    if let Err(error) = std::fs::write(mark, "[ZoneTransfer]\r\nZoneId=3\r\n") {
        log::warn!("a saved file was not marked as downloaded: {:?}", error.kind());
    }
}

/// Linux has no such mark.
#[cfg(not(any(target_os = "macos", windows)))]
const fn mark_as_downloaded(_path: &Path) {}

/// Sends a program past a warning only if the person says so, judged by the name reaching the disk.
pub fn write_out(app: &App, file: &Attachment, data: &[u8]) {
    let name = safety::safe_file_name(&file.name);
    let risk = safety::assess(&name, data);
    if risk.is_some() || safety::may_hold_macros(&name) {
        let text: slint::SharedString = match risk {
            Some(safety::FileRisk::Runnable) => app.invoke_risky_text(1, name.as_str().into()),
            Some(safety::FileRisk::Disguised) => app.invoke_risky_text(2, name.as_str().into()),
            None => crate::settings::say(app, "preview.macros").replace("{name}", &name).into(),
        };
        let yes = asked(
            MessageLevel::Warning,
            &app.get_word_file_warning(),
            &text,
            &app.get_word_file_save(),
            &app.get_word_cancel(),
        );
        if !yes {
            return;
        }
    }
    let Some(path) = FileDialog::new().set_file_name(&name).save_file() else { return };
    match std::fs::write(&path, data) {
        Ok(()) => {
            log::info!("a file was saved");
            mark_as_downloaded(&path);
            app.set_toast(SAVED);
        }
        Err(error) => {
            log::warn!("a file could not be saved: {:?}", error.kind());
            app.set_toast(FAILED);
        }
    }
}

pub fn save_eml() {
    let Some(service) = SERVICE.get().cloned() else { return };
    let Some(message) = view::open_message() else { return };
    let weak = WINDOW.lock().clone();
    tokio::spawn(async move {
        let data = match service.export_eml(&message).await {
            Ok(data) => data,
            Err(error) => {
                log::warn!("a message could not be exported: {}", error.kind());
                toast(FAILED);
                return;
            }
        };
        let name = safety::safe_file_name(&format!("{}.eml", message.meta.subject));
        if let Some(weak) = weak {
            let _ = weak.upgrade_in_event_loop(move |app| {
                let Some(path) = FileDialog::new().set_file_name(&name).save_file() else { return };
                match std::fs::write(&path, data) {
                    Ok(()) => {
                        log::info!("a message was saved as .eml");
                        mark_as_downloaded(&path);
                        app.set_toast(SAVED);
                    }
                    Err(error) => {
                        log::warn!("a message could not be saved: {:?}", error.kind());
                        app.set_toast(FAILED);
                    }
                }
            });
        }
    });
}

/// What can be drawn safely is looked at first, and anything else goes straight to the disk.
fn open_file(index: i32) {
    let Some(service) = SERVICE.get().cloned() else { return };
    let Some(message) = view::open_message() else { return };
    let Ok(index_of_file) = usize::try_from(index) else { return };
    let file = message.attachments.iter().filter(|file| !file.inline).nth(index_of_file).cloned();
    match (file, main_window()) {
        (Some(file), Some(app)) if file_kind::may_be_viewable(file_kind::by_name(&file.name, &file.mime)) => {
            preview::open(&service, &app, &message, file);
        }
        _ => save_file(index),
    }
}

fn save_file(index: i32) {
    let Some(service) = SERVICE.get().cloned() else { return };
    let Some(message) = view::open_message() else { return };
    let Ok(index) = usize::try_from(index) else { return };
    let Some(file) = message.attachments.iter().filter(|file| !file.inline).nth(index).cloned() else { return };
    let weak = WINDOW.lock().clone();
    tokio::spawn(async move {
        let Ok(data) = service.attachment(&message.meta.id, &file, &message.address_id).await else {
            toast(FAILED);
            return;
        };
        if let Some(weak) = weak {
            let _ = weak.upgrade_in_event_loop(move |app| write_out(&app, &file, &data));
        }
    });
}
