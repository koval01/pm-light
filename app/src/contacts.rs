//! The contacts panel. The list stays only while the panel is open: it is the person's address book,
//! and the window has no other use for it.

use crate::{App, ContactRow, ContactsWindow, actions, composer, modal, view};
use proton_core::{Service, compose::is_valid_address, model::ContactEmail, ranking};
use slint::{Color, ComponentHandle, ModelRc, SharedString, Timer, VecModel};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 620.0;

thread_local! {
    static OPEN: RefCell<Option<ContactsWindow>> = const { RefCell::new(None) };
    static ALL: RefCell<Vec<ContactEmail>> = const { RefCell::new(Vec::new()) };
}

fn wanted(contacts: &[ContactEmail], query: &str) -> Vec<usize> {
    let texts: Vec<String> = contacts.iter().map(|contact| format!("{} {}", contact.name, contact.email)).collect();
    ranking::containing(&texts, query.trim())
}

fn show_rows(window: &ContactsWindow) {
    let query = window.get_query();
    let rows: Vec<ContactRow> = ALL.with_borrow(|all| {
        wanted(all, &query)
            .into_iter()
            .filter_map(|at| all.get(at))
            .map(|contact| {
                let name = if contact.name.is_empty() { &contact.email } else { &contact.name };
                ContactRow {
                    id: contact.contact_id.as_str().into(),
                    name: name.as_str().into(),
                    email: contact.email.as_str().into(),
                    initials: view::initials(name).into(),
                    tint: Color::from_argb_encoded(view::OPAQUE | view::tint(name)),
                }
            })
            .collect()
    });
    window.set_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
}

fn reload(service: &Arc<Service>) {
    let service = service.clone();
    tokio::spawn(async move {
        let found = service.contact_emails().await;
        let _ = slint::invoke_from_event_loop(move || {
            OPEN.with_borrow(|open| {
                let Some(window) = open else { return };
                window.set_loading(false);
                if let Ok(found) = found {
                    ALL.set(found);
                    show_rows(window);
                }
            });
        });
    });
}

fn close() {
    let Some(window) = OPEN.take() else { return };
    if let Some(app) = actions::main_window() {
        modal::dismiss(app.window(), window.window());
    }
    let _ = window.hide();
    ALL.with_borrow_mut(Vec::clear);
    Timer::single_shot(Duration::ZERO, move || drop(window));
}

fn add(service: &Arc<Service>, window: &ContactsWindow) {
    let email = window.get_new_email().trim().to_string();
    if !is_valid_address(&email) {
        window.set_problem(window.get_word_bad_address());
        return;
    }
    let typed = window.get_new_name().trim().to_string();
    let name = if typed.is_empty() { email.clone() } else { typed };
    window.set_problem(SharedString::new());
    window.set_new_name(SharedString::new());
    window.set_new_email(SharedString::new());
    let service = service.clone();
    tokio::spawn(async move {
        if service.create_contact(&name, &email).await.is_ok() {
            let _ = slint::invoke_from_event_loop(move || reload(&service));
        } else {
            actions::toast(crate::toasts::FAILED);
        }
    });
}

fn remove(service: &Arc<Service>, id: String) {
    let service = service.clone();
    tokio::spawn(async move {
        if service.delete_contact(&id).await.is_ok() {
            let _ = slint::invoke_from_event_loop(move || reload(&service));
        } else {
            actions::toast(crate::toasts::FAILED);
        }
    });
}

pub fn open(service: &Arc<Service>, app: &App) {
    if OPEN.with_borrow(Option::is_some) {
        return;
    }
    let Ok(window) = ContactsWindow::new() else { return };
    crate::look::dress(&window.global::<crate::Theme>());
    window.set_own_close(cfg!(target_os = "macos"));
    window.on_dismissed(close);
    let weak = window.as_weak();
    window.on_searched(move || {
        if let Some(window) = weak.upgrade() {
            show_rows(&window);
        }
    });
    let (adding, weak) = (service.clone(), window.as_weak());
    window.on_add(move || {
        if let Some(window) = weak.upgrade() {
            add(&adding, &window);
        }
    });
    let writing = service.clone();
    window.on_write(move |email| {
        close();
        composer::start_to(&writing, &email);
    });
    let (pending, weak) = (Rc::new(RefCell::new(String::new())), window.as_weak());
    let chosen = pending.clone();
    window.on_remove(move |id, name| {
        if let Some(window) = weak.upgrade() {
            *chosen.borrow_mut() = id.to_string();
            window.set_confirming(name);
        }
    });
    let (removing, weak) = (service.clone(), window.as_weak());
    window.on_removal_confirmed(move || {
        if let Some(window) = weak.upgrade() {
            window.set_confirming(SharedString::new());
            remove(&removing, pending.borrow().clone());
        }
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
    OPEN.set(Some(window));
    reload(service);
}

/// The window the keyboard is with, when it is this panel.
#[cfg(target_os = "macos")]
pub fn with_window<F: FnOnce(&slint::Window)>(then: F) -> Option<F> {
    let window = OPEN.with_borrow(|open| open.as_ref().map(ComponentHandle::clone_strong));
    match window {
        Some(window) => {
            then(window.window());
            None
        }
        None => Some(then),
    }
}

/// The session is gone, and the address book with it.
pub fn abandon() {
    close();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(name: &str, email: &str) -> ContactEmail {
        ContactEmail { name: name.to_string(), email: email.to_string(), contact_id: String::new() }
    }

    #[test]
    fn a_search_looks_at_the_name_and_the_address() {
        let contacts = [contact("Ann Lee", "ann@example.org"), contact("Bob", "bob@work.net")];
        assert_eq!(wanted(&contacts, "lee"), [0]);
        assert_eq!(wanted(&contacts, "WORK"), [1]);
        assert_eq!(wanted(&contacts, " "), [0, 1]);
        assert_eq!(wanted(&contacts, "zed"), [] as [usize; 0]);
    }
}
