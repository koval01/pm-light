//! What stands around an open message: its recipients, the rest of its conversation, and what the
//! sender asks of the reader. The body itself is `view.rs` and `mail_view.rs`.

use crate::{App, PersonRow, ThreadRow, actions, composer, view};
use proton_core::{
    Service,
    compose,
    crypto::SignatureStatus,
    model::{Address, Message, MessageMeta},
    timing,
};
use slint::{ModelRc, SharedString, VecModel, Weak};
use std::{rc::Rc, sync::Arc};

/// As many as fit on a line. The rest are in the message's details.
const PEOPLE_SHOWN: usize = 4;

fn people(addresses: &[Address]) -> (ModelRc<PersonRow>, i32) {
    let rows: Vec<PersonRow> = addresses
        .iter()
        .take(PEOPLE_SHOWN)
        .map(|address| PersonRow {
            text: address.name.as_deref().unwrap_or(&address.email).into(),
            email: address.email.as_str().into(),
        })
        .collect();
    let left = i32::try_from(addresses.len().saturating_sub(PEOPLE_SHOWN)).unwrap_or(0);
    (ModelRc::from(Rc::new(VecModel::from(rows))), left)
}

/// Everything here belongs to one message, and goes with it when another opens.
pub fn clear(app: &App) {
    fill_people(app, &[], &[]);
    app.set_signature(0);
    app.set_unsubscribable(false);
    app.set_receipt_asked(false);
    app.set_thread(ModelRc::default());
    app.set_thread_open(false);
}

fn fill_people(app: &App, to: &[Address], cc: &[Address]) {
    let (rows, left) = people(to);
    app.set_to_people(rows);
    app.set_to_left(left);
    let (rows, left) = people(cc);
    app.set_cc_people(rows);
    app.set_cc_left(left);
}

pub fn fill(app: &App, message: &Message) {
    fill_people(app, &message.meta.to, &message.cc);
    app.set_signature(match message.signature {
        SignatureStatus::Valid => 1,
        SignatureStatus::Invalid => 2,
        _ => 0,
    });
    app.set_unsubscribable(message.unsubscribe.is_some());
    app.set_receipt_asked(message.receipt_requested);
}

fn thread_rows(rows: &[MessageMeta], open: &str) -> Vec<ThreadRow> {
    rows.iter()
        .map(|row| ThreadRow {
            id: row.id.as_str().into(),
            sender: row.from.name.as_deref().unwrap_or(&row.from.email).into(),
            date: timing::moment_text(i64::try_from(row.date / 1000).unwrap_or(0)).into(),
            current: row.id == open,
        })
        .collect()
}

/// A lone message has no conversation to show, and the question costs one request.
pub fn load_thread(service: &Arc<Service>, app: &Weak<App>, message: &Message) {
    let (service, app) = (service.clone(), app.clone());
    let (conversation, open) = (message.meta.conversation_id.clone(), message.meta.id.clone());
    if conversation.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let Ok(rows) = service.thread(&conversation).await else { return };
        if rows.len() < 2 || view::open_message_id().as_deref() != Some(open.as_str()) {
            return;
        }
        let shown = thread_rows(&rows, &open);
        let _ = app.upgrade_in_event_loop(move |app| {
            if view::open_message_id().as_deref() == Some(open.as_str()) {
                app.set_thread(ModelRc::from(Rc::new(VecModel::from(shown))));
            }
        });
    });
}

pub fn unsubscribe(service: Arc<Service>, app: &App) {
    let Some(message) = view::open_message() else { return };
    let Some(methods) = message.unsubscribe.clone() else { return };
    let url = methods.url.clone().unwrap_or_default();
    let kind = if methods.one_click { 1 } else if url.is_empty() { 0 } else { 2 };
    let text: SharedString = app.invoke_unsubscribe_text(kind, url.as_str().into());
    if !actions::ask_to_unsubscribe(app, &text) {
        return;
    }
    if methods.one_click {
        let id = message.meta.id.clone();
        tokio::spawn(async move {
            let done = service.unsubscribe(&id).await.is_ok();
            actions::toast(if done { crate::toasts::UNSUBSCRIBED } else { crate::toasts::FAILED });
        });
    } else if !url.is_empty() {
        view::open_link(&url);
    } else if let Some(mailto) = methods.mailto {
        composer::start_prepared(&service, compose::compose_prepared(&mailto.to, &mailto.subject, &mailto.body));
    }
}

pub fn send_receipt(service: Arc<Service>, app: &App) {
    let Some(id) = view::open_message_id() else { return };
    app.set_receipt_asked(false);
    tokio::spawn(async move {
        let done = service.send_read_receipt(&id).await.is_ok();
        actions::toast(if done { crate::toasts::RECEIPT_SENT } else { crate::toasts::FAILED });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(name: Option<&str>, email: &str) -> Address {
        Address { name: name.map(str::to_string), email: email.to_string() }
    }

    #[test]
    fn a_long_list_of_people_says_how_many_are_left() {
        let list: Vec<Address> = (0..7).map(|at| person(None, &format!("p{at}@x.org"))).collect();
        let (_, left) = people(&list);
        assert_eq!(left, 3);
        let (_, left) = people(&list[..2]);
        assert_eq!(left, 0);
    }

    #[test]
    fn a_name_stands_for_the_address_but_the_address_is_what_is_kept() {
        use slint::Model;
        let (rows, _) = people(&[person(Some("Ann"), "ann@x.org"), person(None, "bob@x.org")]);
        let first = rows.row_data(0).unwrap_or_default();
        assert_eq!(first.text, "Ann");
        assert_eq!(first.email, "ann@x.org");
        assert_eq!(rows.row_data(1).unwrap_or_default().text, "bob@x.org");
    }
}
