//! The account panel, from the picture at the top right: who is signed in, the plan and the space used, and what can
//! be done with the account from here. The picture shows the account's initials.

use crate::{AccountCard, App, auth, settings};
use proton_core::Service;
use slint::ComponentHandle;
use std::sync::Arc;

/// Up to two letters for the picture: the first of the first two words of the name, or the first of the address.
pub fn initials(name: &str, email: &str) -> String {
    let from_name: String =
        name.split_whitespace().filter_map(|word| word.chars().find(|c| c.is_alphanumeric())).take(2).collect();
    let letters = if from_name.is_empty() {
        email.chars().find(|c| c.is_alphanumeric()).map_or_default(String::from)
    } else {
        from_name
    };
    letters.to_uppercase()
}

/// The picture's letters, once the account's addresses are known.
pub fn dress(service: &Service, app: &App) {
    let (name, email) = first_address(service);
    app.set_account_initials(initials(&name, &email).into());
}

fn first_address(service: &Service) -> (String, String) {
    service.addresses().first().map_or_default(|address| (address.display_name.clone(), address.email.clone()))
}

fn open(service: &Arc<Service>, app: &App) {
    // Asked again while open: it is open already, and covering the mail twice would leave it covered.
    if app.get_account_open() {
        return;
    }
    let (name, email) = first_address(service);
    app.set_account_initials(initials(&name, &email).into());
    app.set_account_card(AccountCard {
        initials: initials(&name, &email).into(),
        name: name.into(),
        email: email.into(),
        can_lock: service.lock_key().is_some(),
        ..AccountCard::default()
    });
    // The mail's page sits above everything the toolkit draws, the panel too.
    crate::mail_view::covered(true);
    app.set_account_open(true);
    log::info!("the account panel opened");
    let (service, weak) = (service.clone(), app.as_weak());
    tokio::spawn(async move {
        let account = service.account().await;
        let wording = service.clone();
        let storage = service.storage().await.ok();
        let _ = weak.upgrade_in_event_loop(move |app| {
            let Ok(account) = account else {
                log::warn!("the account panel: the account did not load");
                return;
            };
            let mut card = app.get_account_card();
            let (plan, space) = settings::account_lines(&wording, &app, &account, storage.as_ref());
            card.plan = plan.into();
            card.storage = space.into();
            card.share = storage.map_or(0.0, |storage| crate::numbers::share(storage.used, storage.max));
            if !account.name.is_empty() && card.name.is_empty() {
                card.name = account.name.as_str().into();
            }
            card.loaded = true;
            app.set_account_card(card);
        });
    });
}

fn chosen(service: &Arc<Service>, app: &App, choice: &str) {
    if app.get_account_open() {
        crate::mail_view::covered(false);
    }
    app.set_account_open(false);
    match choice {
        "settings" => settings::open(service, app, "overview"),
        "lock" => {
            if service.lock_now() {
                auth::locked(service, app);
            }
        }
        "sign-out" => auth::ask_to_sign_out(service, app),
        _ => {}
    }
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let (opening, weak) = (service.clone(), app.as_weak());
    app.on_account_requested(move || {
        if let Some(app) = weak.upgrade() {
            open(&opening, &app);
        }
    });
    let (choosing, weak) = (service.clone(), app.as_weak());
    app.on_account_chosen(move |choice| {
        if let Some(app) = weak.upgrade() {
            chosen(&choosing, &app, &choice);
        }
    });
}

#[cfg(test)]
mod tests;
