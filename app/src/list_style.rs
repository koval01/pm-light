//! A folder a page at a time, as Proton's own client shows it, or one list that grows as it is scrolled. Pages are
//! the usual way; which of the two is asked once, after the mailbox first opened and after the tour, and kept on
//! this computer. The settings change it later.

use crate::{App, preferences, tray};
use proton_core::Service;
use slint::{ComponentHandle, Timer};
use std::{sync::Arc, time::Duration};

/// Long enough for the first folder to be drawn under the question.
const AFTER_THE_MAILBOX: Duration = Duration::from_millis(900);
/// A menu of pages stays short enough to use: past this many, it holds those around the page on screen, and the
/// first and the last.
const LISTED: u64 = 200;

/// Whether folders are shown a page at a time: pages until the person chose otherwise.
#[must_use]
pub fn paged() -> bool {
    preferences::get().list_pages.unwrap_or(true)
}

/// After the mailbox opened, or the tour ended: the question, if it was never answered.
pub fn offer(app: &App) {
    let chosen = preferences::get();
    if chosen.list_pages.is_some() || chosen.tour_pending {
        return;
    }
    let weak = app.as_weak();
    Timer::single_shot(AFTER_THE_MAILBOX, move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_asking_list_style() || app.get_tour_step() > 0 || preferences::get().list_pages.is_some() {
            return;
        }
        // The mail's page sits above everything the toolkit draws: out of sight while the question is on.
        crate::mail_view::covered(true);
        app.set_asking_list_style(true);
        log::info!("the list's style was asked");
    });
}

/// The pages a menu lists, from 0: every one, or those around `current` with the first and the last.
#[must_use]
pub fn listed(current: u64, pages: u64) -> Vec<u64> {
    if pages <= LISTED {
        return (0..pages).collect();
    }
    let from = current.saturating_sub(LISTED / 2).min(pages - LISTED);
    let mut listed: Vec<u64> = (from..from + LISTED).collect();
    if from > 0 {
        listed.insert(0, 0);
    }
    if from + LISTED < pages {
        listed.push(pages - 1);
    }
    listed
}

fn go(service: &Arc<Service>, page: u64) {
    let service = service.clone();
    tokio::spawn(async move { service.view_page(page).await });
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let choosing = service.clone();
    app.on_list_style_chosen(move |pages| {
        crate::mail_view::covered(false);
        preferences::change(|preferences| preferences.list_pages = Some(pages));
        log::info!("the list is shown {}", if pages { "in pages" } else { "as one list" });
        let service = choosing.clone();
        tokio::spawn(async move { service.view_paged(pages).await });
    });
    let stepping = service.clone();
    app.on_go_to_page(move |page| go(&stepping, u64::try_from(page).unwrap_or(0)));
    let (picking, weak) = (service.clone(), app.as_weak());
    app.on_choose_page(move || {
        let Some(app) = weak.upgrade() else { return };
        let (current, pages) =
            (u64::try_from(app.get_list_page()).unwrap_or(0), u64::try_from(app.get_list_pages()).unwrap_or(0));
        let pages = listed(current, pages);
        let names: Vec<String> = pages.iter().map(|page| (page + 1).to_string()).collect();
        let marked = pages.iter().position(|page| *page == current);
        let service = picking.clone();
        tray::choose_marked(&app, &names, marked, move |_, position| {
            if let Some(page) = pages.get(position) {
                go(&service, *page);
            }
        });
    });
}

#[cfg(test)]
mod tests;
