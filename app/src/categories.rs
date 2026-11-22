//! The inbox's categories as tabs above the list: which the account shows, what is unread in each, and a click on one,
//! which opens the inbox at that category as Proton's own tab does, filters and the open mail let go.

use crate::{App, CategoryTab, actions, mail_view, view};
use proton_core::{Service, categories, model::Counts};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{rc::Rc, sync::Arc};

pub fn connect(service: &Arc<Service>, app: &App) {
    let (service, weak) = (service.clone(), app.as_weak());
    app.on_category_chosen(move |id| {
        let id = id.to_string();
        // Proton's own word for it: the id is a number of Proton's, and names nothing of the person's.
        log::info!("opening the category {}", categories::name_of(&id).unwrap_or("unknown"));
        mail_view::close();
        view::let_go_of_mail();
        if let Some(app) = weak.upgrade() {
            app.set_has_mail(false);
        }
        let service = service.clone();
        tokio::spawn(async move { service.view_category(&id).await });
    });
}

/// What a move target that is a category starts with: no folder's id holds a colon.
pub const TARGET: &str = "category:";

/// Where the open or ticked mail may go, as Proton's move menu has it: in the inbox shown by category, the other tabs
/// stand where the inbox would.
#[must_use]
pub fn move_targets(app: &App) -> Vec<(String, String)> {
    let open = app.get_category();
    let folders = view::move_targets();
    if open.is_empty() {
        return folders;
    }
    let words = app.global::<crate::Categories>();
    let tabs: Vec<(String, String)> = app
        .get_category_tabs()
        .iter()
        .filter(|tab| tab.id != open)
        .map(|tab| (format!("{TARGET}{}", tab.id), words.invoke_title(tab.name).to_string()))
        .collect();
    let mut targets = Vec::new();
    for (id, name) in folders {
        if id == "inbox" {
            targets.extend(tabs.iter().cloned());
        } else {
            targets.push((id, name));
        }
    }
    targets
}

/// The tabs as the account has them, each with its unread mail. None where the inbox is not shown by category.
#[must_use]
pub fn tabs(categories: &categories::Categories, counts: &Counts) -> Vec<CategoryTab> {
    if !categories.on() {
        return Vec::new();
    }
    categories
        .tabs()
        .into_iter()
        .map(|tab| CategoryTab {
            id: tab.id.into(),
            name: tab.name.into(),
            unread: i32::try_from(categories.unread(tab.id, counts)).unwrap_or(i32::MAX),
        })
        .collect()
}

/// Whenever new counts come, the tabs are drawn again with them.
pub fn show_counts(app: &App, counts: &Counts) {
    let Some(service) = actions::service() else { return };
    let tabs = tabs(&service.categories(), counts);
    app.set_category_tabs(ModelRc::from(Rc::new(VecModel::from(tabs))));
}

#[cfg(test)]
mod tests;
