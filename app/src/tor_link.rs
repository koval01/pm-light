//! Tor as the app uses it: asked about at the first start, chosen once for each start, and shown at the foot of the
//! sidebar and on the splash.
//!
//! Nothing goes to Proton before the way is chosen. With Tor on, sign-in begins at once and its first request waits
//! for Tor; if Tor cannot start, the app stays offline and says so, and never reaches Proton another way.

use crate::{App, auth, guard, preferences, settings};
use proton_core::{
    Service,
    client::Way,
    tor::{Hop, Route, Tor, TorState, TorStatus},
};
use slint::{ComponentHandle, Weak};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

/// What the window draws of Tor's state.
pub const SHOWN_OFF: i32 = 0;
pub const SHOWN_STARTING: i32 = 1;
pub const SHOWN_READY: i32 = 2;
pub const SHOWN_FAILED: i32 = 3;

static TOR: OnceLock<Arc<Tor>> = OnceLock::new();

/// Tor, when this start goes through it and it has started.
pub fn tor() -> Option<Arc<Tor>> {
    TOR.get().cloned()
}

/// Whether this start goes through Tor at all, started or not.
pub fn chosen() -> bool {
    preferences::get().tor.is_some_and(|choice| choice.on)
}

/// Before anything goes out: the question, or straight on along the way chosen before.
///
/// The question comes whenever Tor has never been asked about, and not only at an app's very first start: someone
/// who used a version without Tor is told about it, and offered it, the first time a version with Tor starts.
pub fn begin(service: &Arc<Service>, app: &App) {
    let (answering, weak) = (service.clone(), app.as_weak());
    app.on_answer_tor(move |on, onion| {
        let route = if onion { Route::Onion } else { Route::Exit };
        let choice = preferences::TorChoice { on, route };
        preferences::change(|preferences| preferences.tor = Some(choice));
        log::info!("the question about Tor: {}", if on { "on" } else { "off" });
        if let Some(app) = weak.upgrade() {
            app.set_asking_tor(false);
        }
        go(&answering, &weak, choice);
    });
    if let Some(choice) = preferences::get().tor {
        go(service, &app.as_weak(), choice);
    } else if crate::account_check::asked() {
        // A check run by a machine: straight to Proton unless its long run asks for Tor, and nothing kept as the
        // person's answer.
        go(service, &app.as_weak(), preferences::TorChoice { on: crate::soak::through_tor(), route: Route::default() });
    } else {
        log::info!("Tor has not been asked about yet");
        app.set_asking_tor(true);
    }
}

fn go(service: &Arc<Service>, app: &Weak<App>, choice: preferences::TorChoice) {
    if !choice.on {
        if let Err(error) = service.choose_way(&Way::Direct) {
            log::error!("the way to Proton was not set: {}", error.kind());
        }
        show(app, SHOWN_OFF, "");
        auth::start(service, app);
        return;
    }
    show(app, SHOWN_STARTING, &line(&TorStatus::default(), &words(app)));
    let (service, app) = (service.clone(), app.clone());
    tokio::spawn(async move {
        let Some(folder) = guard::tor_dir() else {
            log::error!("Tor has no folder to keep its state in");
            show(&app, SHOWN_FAILED, &words(&app).failed);
            return;
        };
        let tor = match Tor::start(&folder).await {
            Ok(tor) => tor,
            Err(error) => {
                log::error!("Tor could not start: {}", error.kind());
                show(&app, SHOWN_FAILED, &words(&app).failed);
                return;
            }
        };
        let _ = TOR.set(tor.clone());
        let (watching, settings_service) = (app.clone(), service.clone());
        tor.on_status(Box::new(move |status| {
            let (status, service) = (status.clone(), settings_service.clone());
            let _ = watching.upgrade_in_event_loop(move |window| {
                let shown = match status.state {
                    TorState::Starting => SHOWN_STARTING,
                    TorState::Ready => SHOWN_READY,
                    TorState::Failed => SHOWN_FAILED,
                };
                window.set_tor_state(shown);
                window.set_tor_text(line(&status, &words_of(&window)).into());
                settings::tor_changed(&service, &window);
            });
        }));
        match service.choose_way(&Way::Tor { tor, route: choice.route }) {
            Ok(()) => auth::start(&service, &app),
            Err(error) => log::error!("the way through Tor was not set: {}", error.kind()),
        }
    });
}

fn show(app: &Weak<App>, state: i32, text: &str) {
    let text = text.to_string();
    let _ = app.upgrade_in_event_loop(move |app| {
        app.set_tor_state(state);
        app.set_tor_text(text.into());
    });
}

/// The words a line is made of, in the window's language.
pub struct Words {
    pub starting: String,
    pub ready: String,
    pub ready_alone: String,
    pub failed: String,
}

fn words_of(app: &App) -> Words {
    Words {
        starting: settings::say(app, "tor.starting"),
        ready: settings::say(app, "tor.ready"),
        ready_alone: settings::say(app, "tor.readyAlone"),
        failed: settings::say(app, "tor.failed"),
    }
}

fn words(app: &Weak<App>) -> Words {
    app.upgrade().map_or_else(
        || Words { starting: String::new(), ready: String::new(), ready_alone: String::new(), failed: String::new() },
        |app| words_of(&app),
    )
}

/// One line for the foot of the sidebar: how far Tor has come, or the countries of the circuit to Proton.
pub fn line(status: &TorStatus, words: &Words) -> String {
    match status.state {
        TorState::Starting => words.starting.replace("{percent}", &status.percent.to_string()),
        TorState::Failed => words.failed.clone(),
        TorState::Ready if status.circuit.is_empty() => words.ready_alone.clone(),
        TorState::Ready => words.ready.replace("{path}", &status.circuit.join(" → ")),
    }
}

/// Tor turned off, by the person's choice after a warning, and the app started again without it: no request of
/// this start goes out another way.
pub fn leave() {
    preferences::change(|preferences| {
        let route = preferences.tor.map_or_default(|choice| choice.route);
        preferences.tor = Some(preferences::TorChoice { on: false, route });
    });
    guard::relaunch();
}

/// A relay on the connection page: its country, and its address when the directory gave one.
pub fn hop_text(hop: &Hop) -> String {
    if hop.address.is_empty() { hop.country.clone() } else { format!("{} · {}", hop.country, hop.address) }
}

static RENEWING: AtomicBool = AtomicBool::new(false);

/// Whether new circuits are being built: until Proton has been reached over them, no others are asked for.
pub fn renewing() -> bool {
    RENEWING.load(Ordering::Relaxed)
}

/// New circuits for everything from now on, the open connections cut, and Proton reached over the new circuit at
/// once, so that the window shows it. Done when that request has come back, either way.
pub async fn new_circuits(service: Arc<Service>) -> proton_core::error::Result<()> {
    let Some(tor) = tor() else { return Ok(()) };
    if RENEWING.swap(true, Ordering::Relaxed) {
        return Ok(());
    }
    tor.new_circuits();
    let started = std::time::Instant::now();
    let reached = service.ping().await;
    RENEWING.store(false, Ordering::Relaxed);
    match &reached {
        Ok(()) => log::info!("Tor: Proton reached over the new circuit in {} ms", started.elapsed().as_millis()),
        Err(error) => log::warn!("Tor: Proton not reached over the new circuit: {}", error.kind()),
    }
    reached
}

#[cfg(test)]
mod tests;
