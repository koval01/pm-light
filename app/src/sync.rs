//! Keeps the mailbox current whether or not the window is on screen: this thread does the work
//! itself, so a hidden window cannot delay a notification.

use crate::{App, actions, autolock, notifications, view};
use parking_lot::Mutex;
use proton_core::{
    Service,
    error::{Error, Link},
};
use slint::{ComponentHandle, Weak};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime},
};
use tokio::runtime::Handle;

/// The cadence of Proton's own clients. Faster polling would only invite rate limits.
const PERIOD: Duration = Duration::from_secs(30);
const HEARTBEAT: Duration = Duration::from_secs(5);
/// A heartbeat this late means the machine slept: `sleep` stops with it, the wall clock does not.
const WAKE_GAP: Duration = Duration::from_secs(20);
/// Focus, waking and the menu may all ask at once.
const MIN_INTERVAL: Duration = Duration::from_secs(5);

static LAST_CHECK: Mutex<Option<Instant>> = Mutex::new(None);
static LINK: Mutex<Link> = Mutex::new(Link::Online);

fn due(since_heartbeat: Duration, since_sync: Duration) -> bool {
    since_heartbeat >= WAKE_GAP || since_sync >= PERIOD
}

fn elapsed(since: SystemTime) -> Duration {
    since.elapsed().unwrap_or_default() // a clock moved backwards counts as no time
}

fn claim_check(last: &mut Option<Instant>, now: Instant) -> bool {
    if last.is_some_and(|at| now.duration_since(at) < MIN_INTERVAL) {
        return false;
    }
    *last = Some(now);
    true
}

/// The new state, if this is news.
fn change_link(known: &mut Link, link: Link) -> Option<Link> {
    (std::mem::replace(known, link) != link).then_some(link)
}

const fn link_code(link: Link) -> i32 {
    match link {
        Link::Online => 0,
        Link::Offline => 1,
        Link::Unavailable => 2,
        Link::Untrusted => 3,
    }
}

fn report_link(app: &Weak<App>, link: Link) {
    let changed = change_link(&mut LINK.lock(), link);
    if let Some(link) = changed {
        log::info!("connection to Proton: {link:?}");
        let _ = app.upgrade_in_event_loop(move |app| app.set_link(link_code(link)));
    }
}

async fn check(service: Arc<Service>, app: Weak<App>) {
    if !service.is_signed_in() {
        return;
    }
    match service.check_mail().await {
        Ok(None) => {
            report_link(&app, Link::Online);
            service.view_confirmed();
        }
        Ok(Some(change)) => {
            report_link(&app, Link::Online);
            log::debug!("mail changed: {} new, {} unread", change.new_mail, change.unread);
            let _ = app.upgrade_in_event_loop(move |app| {
                view::show_counts(&app, &change.counts);
                if change.new_mail > 0 {
                    notifications::announce(&app, change.new_mail);
                }
            });
            service.view_reload().await;
        }
        Err(Error::SessionEnded | Error::Cancelled) => {}
        Err(error) => match error.link() {
            Some(link) => report_link(&app, link),
            None => log::warn!("mail check failed: {}", error.kind()),
        },
    }
}

/// Asked for by focus, the menu and a finished sign-in.
pub fn request_check() {
    let (Some(service), Some(app)) = (actions::service(), actions::main_window()) else { return };
    if claim_check(&mut LAST_CHECK.lock(), Instant::now()) {
        tokio::spawn(check(service, app.as_weak()));
    }
}

fn beat(service: &Arc<Service>, app: &Weak<App>, runtime: &Handle, heartbeat: SystemTime, synced: &mut SystemTime) {
    let since_heartbeat = elapsed(heartbeat);
    autolock::check(service, app, if since_heartbeat >= WAKE_GAP { since_heartbeat } else { Duration::ZERO });
    // Offline, the light ping notices the moment Proton is back. Online, the mail check itself
    // proves the connection, so nothing is polled for nothing.
    if *LINK.lock() == Link::Offline {
        match runtime.block_on(service.ping()) {
            Ok(()) => {
                report_link(app, Link::Online);
                runtime.block_on(check(service.clone(), app.clone()));
            }
            Err(error) => report_link(app, error.link().unwrap_or(Link::Offline)),
        }
    } else if due(elapsed(heartbeat), elapsed(*synced)) {
        if claim_check(&mut LAST_CHECK.lock(), Instant::now()) {
            runtime.block_on(check(service.clone(), app.clone()));
        }
        *synced = SystemTime::now();
    }
}

pub fn start(service: &Arc<Service>, app: &App) {
    let (service, app, runtime) = (service.clone(), app.as_weak(), Handle::current());
    let spawned = thread::Builder::new().name("mail-sync".into()).spawn(move || {
        let mut heartbeat = SystemTime::now();
        let mut synced = heartbeat;
        loop {
            thread::sleep(HEARTBEAT);
            beat(&service, &app, &runtime, heartbeat, &mut synced);
            heartbeat = SystemTime::now();
        }
    });
    if spawned.is_err() {
        log::error!("no background sync");
    }
}

#[cfg(test)]
mod tests;
