//! One broken part must not take the app with it. A panic on a worker ends only that task: the
//! window is freed, so that nothing waits on an answer that will not come. A panic on the window's
//! thread cannot be gone on from inside the event loop, so the program starts itself again, and a
//! letter being written survives as the draft it was last saved to.

use crate::{App, Auth, actions, composer};
use proton_core::journal;
use slint::ComponentHandle;
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const ARGUMENT: &str = "--restarted=";
const MAIN_THREAD: &str = "main";
/// Restarts this close together mean the start itself is broken, and another would only repeat it.
const RESTART_WINDOW_SECONDS: u64 = 120;
const MOST_RESTARTS: u32 = 3;

pub const IDENTIFIER: &str = "org.koval-dev.proton-mail-light";
const MAIL_FOLDER: &str = "mail";

static SAID: AtomicBool = AtomicBool::new(false);

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_secs())
}

fn parsed(argument: &str) -> Option<(u32, u64)> {
    let (count, at) = argument.strip_prefix(ARGUMENT)?.split_once(':')?;
    Some((count.parse().ok()?, at.parse().ok()?))
}

/// The argument for the next start, or nothing if this has gone on for too long.
fn next_restart(previous: Option<&str>, now: u64) -> Option<String> {
    let earlier = previous.and_then(parsed);
    let count =
        earlier.filter(|(_, at)| now.saturating_sub(*at) <= RESTART_WINDOW_SECONDS).map_or(0, |(count, _)| count);
    (count < MOST_RESTARTS).then(|| format!("{ARGUMENT}{}:{now}", count + 1))
}

fn this_start() -> Option<String> {
    std::env::args().find(|argument| argument.starts_with(ARGUMENT))
}

pub fn is_restart() -> bool {
    this_start().is_some()
}

/// A start asked for on purpose, such as after Tor was turned on or off: the app quits, and starts once it is gone.
const RELAUNCH: &str = "--relaunched";
static RELAUNCH_ASKED: AtomicBool = AtomicBool::new(false);

/// Quits, and starts the app again once this copy has let go of everything.
pub fn relaunch() {
    log::info!("starting again on purpose");
    RELAUNCH_ASKED.store(true, Ordering::Relaxed);
    let _ = slint::quit_event_loop();
}

/// After the event loop: the new copy starts only now, so the two never run side by side.
pub fn relaunch_if_asked() {
    if !RELAUNCH_ASKED.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(program) = std::env::current_exe()
        && let Err(error) = Command::new(program).arg(RELAUNCH).spawn()
    {
        log::error!("the app could not start again: {:?}", error.kind());
    }
}

/// This copy was started by `relaunch`, while the one before it was still on its way out.
pub fn is_relaunch() -> bool {
    std::env::args().any(|argument| argument == RELAUNCH)
}

/// Once, and only when the mail is on screen: the splash would hide it.
pub fn say_if_restarted(app: &App) {
    if this_start().is_some() && !SAID.swap(true, Ordering::Relaxed) {
        app.set_toast(crate::toasts::RESTARTED);
    }
}

fn start_again() {
    let Some(argument) = next_restart(this_start().as_deref(), now()) else { return };
    if let Ok(program) = std::env::current_exe() {
        let _ = Command::new(program).arg(argument).spawn();
    }
}

/// Frees whatever waited on the task that died.
fn recover() {
    composer::after_failure();
    let Some(app) = actions::main_window() else { return };
    free_window(&app);
    app.set_toast(crate::toasts::INTERNAL);
}

fn free_window(app: &App) {
    app.set_loading(false);
    app.set_paging(false);
    if app.get_opening() {
        app.set_opening(false);
        app.set_has_mail(false);
    }
    app.global::<Auth>().set_busy(false);
}

/// Only where it happened: a panic's message can quote the mail that caused it.
pub fn install() {
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map_or_else(String::new, |at| format!("{}:{}", at.file(), at.line()));
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        log::error!(target: "proton_panic", "panic on {thread} at {place}");
        log::logger().flush();
        if thread == MAIN_THREAD {
            start_again();
            // Skips the clean-up on purpose: the record is what explains this. The window's
            // thread cannot go on after a panic, and the copy just started takes over.
            #[expect(clippy::exit, reason = "a panic on the window's thread ends this copy of the program")]
            std::process::exit(101);
        }
        let _ = slint::invoke_from_event_loop(recover);
    }));
}

fn data_dir() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(IDENTIFIER))
}

/// Where Tor keeps its guards and its copy of the network's directory. It stays between starts, and quitting does
/// not erase it: new guards at every start are what would make a person easier to follow.
pub fn tor_dir() -> Option<PathBuf> {
    Some(data_dir()?.join("tor"))
}

/// Mail on disk, sealed with a key that lives in the session. Signing out erases it.
pub fn use_mail_store(service: &proton_core::Service) {
    if let Some(dir) = data_dir() {
        let mail = dir.join(MAIL_FOLDER);
        let kept = crate::preferences::get().mail_on_disk.then(|| mail.clone());
        service.use_mail_dir(kept, Some(&mail));
    }
}

pub fn start_journal() {
    if let Some(dir) = data_dir() {
        journal::start(dir.join("native-journal"), crate::session::journal_key);
    }
}

/// Quitting takes the journal and the webview's caches along, for whoever forgets to come back.
pub fn leave_no_trace() {
    journal::clear();
    if let Some(cache) = dirs::cache_dir().map(|dir| dir.join(IDENTIFIER)) {
        let _ = std::fs::remove_dir_all(cache);
    }
}

#[cfg(test)]
mod tests;
