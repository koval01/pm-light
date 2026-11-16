//! Calling a sent message back: for the moment its sending can still be undone, the window offers it, and taking it
//! back opens it again as a draft.

use crate::{App, composer, view::failed_toast};
use parking_lot::Mutex;
use proton_core::Service;
use slint::ComponentHandle;
use std::sync::Arc;

static UNDOABLE: Mutex<Option<String>> = Mutex::new(None);

/// The message went out, and can still be called back for a moment.
pub fn offer(id: String, seconds: u64) {
    *UNDOABLE.lock() = Some(id);
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = crate::actions::main_window() {
            app.set_undo_seconds(i32::try_from(seconds).unwrap_or(0));
        }
    });
}

fn undo(service: &Arc<Service>, app: &App) {
    app.set_undo_seconds(0);
    let Some(id) = UNDOABLE.lock().take() else { return };
    let (service, weak, labels) = (service.clone(), app.as_weak(), composer::labels(app));
    tokio::spawn(async move {
        if service.cancel_send(&id).await.is_err() {
            failed_toast(&weak);
            return;
        }
        let (drafting, opening) = (service.clone(), weak.clone());
        let _ = slint::invoke_from_event_loop(move || composer::start_draft(&drafting, &opening, id, labels));
        service.view_reload().await;
    });
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let (service, weak) = (service.clone(), app.as_weak());
    app.on_undo_send(move || {
        if let Some(app) = weak.upgrade() {
            undo(&service, &app);
        }
    });
}
