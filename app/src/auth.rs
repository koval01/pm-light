//! Getting into the account: the saved session, the sign-in steps, the app password, and what
//! happens when a session ends. Rules from `docs/DEVELOPMENT.md` hold here: a failed sign-in is never tried
//! again by itself, and the words Proton answered with go to the person, never to a log.

use crate::{App, Auth, actions, composer, guard, incoming, keyboard, preview, session, settings, sync, tray, verify, view};
use proton_core::{
    AuthStep, Service,
    client::Ending,
    error::{Error, Link},
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, VecModel, Weak};
use std::{future::Future, rc::Rc, sync::Arc, time::Duration};
use zeroize::Zeroizing;

const STEP_NONE: i32 = 0;
const STEP_CREDENTIALS: i32 = 1;
const STEP_CODE: i32 = 2;
const STEP_MAILBOX: i32 = 3;
const STEP_LOCKED: i32 = 4;
const STEP_WAITING: i32 = 5;
const STEP_VERIFY: i32 = 6;
const CODE_LENGTH: usize = 6;
const LAID_OUT: Duration = Duration::from_millis(50);
pub const FIRST_FOLDER: &str = "inbox";

const ERROR_INTERNAL: i32 = 1;
const ERROR_UNTRUSTED: i32 = 2;
const ERROR_NETWORK: i32 = 3;
const ERROR_WRONG_PASSWORD: i32 = 4;
const ERROR_UNAVAILABLE: i32 = 5;
const ERROR_SAID: i32 = 6;

/// What to say about a failure, as `describeError` does in `main`: a code for words the window
/// knows, or the server's own words, which explain more than a status.
fn error_of(error: &Error) -> (i32, String) {
    match error {
        Error::Cancelled | Error::TooManyAttempts { .. } | Error::SessionEnded => (0, String::new()),
        Error::Untrusted => (ERROR_UNTRUSTED, String::new()),
        Error::Network(_) => (ERROR_NETWORK, String::new()),
        Error::WrongPassword => (ERROR_WRONG_PASSWORD, String::new()),
        Error::Api { status, code, message } if *code != 0 || !message.is_empty() => {
            (ERROR_SAID, format!("{message} (code {code}, HTTP {status})"))
        }
        Error::Api { .. } if error.link() == Some(Link::Unavailable) => (ERROR_UNAVAILABLE, String::new()),
        Error::Other(message) | Error::Crypto(message) => (ERROR_SAID, message.clone()),
        _ => (ERROR_INTERNAL, String::new()),
    }
}

/// The cells of a two-factor code: the window cannot cut a string itself.
fn show_code(auth: &Auth<'_>, code: &str) {
    if auth.get_code() != code {
        auth.set_code(code.into());
    }
    let cells: Vec<SharedString> = code.chars().map(|digit| digit.to_string().into()).collect();
    auth.set_code_cells(ModelRc::from(Rc::new(VecModel::from(cells))));
}

fn clear_fields(auth: &Auth<'_>) {
    auth.set_password(SharedString::new());
    show_code(auth, "");
    auth.set_recovery(SharedString::new());
    auth.set_mailbox(SharedString::new());
    auth.set_lock_password(SharedString::new());
}

fn show_step(app: &App, step: i32) {
    let auth = app.global::<Auth>();
    if step != STEP_VERIFY {
        verify::close();
    }
    auth.set_error(0);
    auth.set_error_text(SharedString::new());
    auth.set_busy(false);
    clear_fields(&auth);
    auth.set_step(step);
    view::reveal(app);
}

fn show_error(app: &App, error: &Error) {
    let (code, text) = error_of(error);
    let auth = app.global::<Auth>();
    auth.set_busy(false);
    auth.set_error(code);
    auth.set_error_text(text.into());
    if let Error::TooManyAttempts { seconds } = error {
        auth.set_frozen(i32::try_from(*seconds).unwrap_or(i32::MAX));
    }
    show_code(&auth, "");
}

/// Runs one step, and takes the next from its answer. A failure stays on the screen it came from.
fn act(service: &Arc<Service>, app: &App, step: impl Future<Output = proton_core::error::Result<AuthStep>> + Send + 'static) {
    let auth = app.global::<Auth>();
    auth.set_busy(true);
    auth.set_error(0);
    auth.set_error_text(SharedString::new());
    let (service, weak) = (service.clone(), app.as_weak());
    tokio::spawn(async move {
        let answer = step.await;
        go_on(&service, &weak, answer);
    });
}

fn go_on(service: &Arc<Service>, weak: &Weak<App>, answer: proton_core::error::Result<AuthStep>) {
    let (service, again) = (service.clone(), weak.clone());
    let _ = weak.upgrade_in_event_loop(move |app| match answer {
        Ok(step) => apply(&service, &app, &again, step),
        Err(error) => show_error(&app, &error),
    });
}

fn apply(service: &Arc<Service>, app: &App, weak: &Weak<App>, step: AuthStep) {
    match step {
        AuthStep::Ready => {
            show_step(app, STEP_NONE);
            crate::account_panel::dress(service, app);
            crate::tour::offer(app);
            crate::list_style::offer(app);
            app.global::<Auth>().set_session_ended(false);
            open_mailbox(service, weak);
            guard::say_if_restarted(app);
        }
        AuthStep::Credentials => show_step(app, STEP_CREDENTIALS),
        AuthStep::Totp => show_step(app, STEP_CODE),
        AuthStep::Mailbox => show_step(app, STEP_MAILBOX),
        AuthStep::Locked => show_locked(service, app),
        AuthStep::Waiting { link } => wait_for(service, app, weak, link),
        AuthStep::Verify { challenge } => show_verification(service, app, weak, &challenge.web_url),
    }
}

/// The app locked itself: nothing of the mailbox stays on screen until it is opened again.
pub fn locked(service: &Arc<Service>, app: &App) {
    incoming::mailbox_closed();
    preview::close();
    composer::abandon();
    settings::abandon(app);
    view::forget_mailbox(app);
    tray::show_unread(app, 0);
    show_locked(service, app);
}

fn show_locked(service: &Arc<Service>, app: &App) {
    show_step(app, STEP_LOCKED);
    let auth = app.global::<Auth>();
    auth.set_touch_id(session::touch_id::available() && session::has_unlock_key());
    auth.set_frozen(i32::try_from(service.unlock_wait()).unwrap_or(i32::MAX));
}

/// Keeps trying by itself, as often as the kind of trouble deserves. The screen offers the way out.
fn wait_for(service: &Arc<Service>, app: &App, weak: &Weak<App>, link: Link) {
    show_step(app, STEP_WAITING);
    app.global::<Auth>().set_waiting_for(match link {
        Link::Unavailable => 1,
        Link::Untrusted => 2,
        _ => 0,
    });
    app.global::<Auth>().set_through_tor(service.through_tor());
    let (service, weak) = (service.clone(), weak.clone());
    Timer::single_shot(link.retry_delay(), move || {
        let waiting = weak.upgrade().is_some_and(|app| app.global::<Auth>().get_step() == STEP_WAITING);
        if waiting {
            try_again(&service, &weak);
        }
    });
}

/// Asks Proton again for the saved session. The saved session is never given up here: being offline is no reason
/// to sign in again.
fn try_again(service: &Arc<Service>, weak: &Weak<App>) {
    let (service, weak) = (service.clone(), weak.clone());
    tokio::spawn(async move {
        let step = service.reopen().await;
        go_on(&service, &weak, Ok(step));
    });
}

fn verification_ended(service: &Arc<Service>, app: &App, solved: Option<(String, String)>) {
    verify::close();
    let (service_too, weak) = (service.clone(), app.as_weak());
    app.global::<Auth>().set_busy(true);
    tokio::spawn(async move {
        let answer = service_too.continue_after_verification(solved).await;
        let (service, again) = (service_too.clone(), weak.clone());
        let _ = weak.upgrade_in_event_loop(move |app| match answer {
            Ok(step) => apply(&service, &app, &again, step),
            // A wrong password cannot be fixed from the verification screen.
            Err(error) => {
                show_step(&app, STEP_CREDENTIALS);
                show_error(&app, &error);
            }
        });
    });
}

fn show_verification(service: &Arc<Service>, app: &App, weak: &Weak<App>, address: &str) {
    show_step(app, STEP_VERIFY);
    let auth = app.global::<Auth>();
    auth.set_verify_address(address.into());
    auth.set_verify_in_browser(false);
    let language = sys_locale::get_locale().unwrap_or_default();
    let language = language.split(['-', '_']).next().unwrap_or("en").to_lowercase();
    let page = verify::page_url(address, &language, auth.get_dark());
    let (service, weak) = (service.clone(), weak.clone());
    // After a frame: the space for the page does not exist until the screen has been laid out, and a
    // webview opened into nothing stays invisible.
    Timer::single_shot(LAID_OUT, move || {
        let Some(app) = weak.upgrade() else { return };
        let reporting = weak.clone();
        let opened = page.is_some_and(|page| {
            verify::open(&app, &page, move |outcome| {
                let Some(app) = reporting.upgrade() else { return };
                match outcome {
                    verify::Outcome::Solved(solved) => verification_ended(&service, &app, solved),
                    verify::Outcome::Closed => back_to_credentials(&service, &app),
                }
            })
        });
        app.global::<Auth>().set_verify_failed(!opened);
    });
}

fn back_to_credentials(service: &Arc<Service>, app: &App) {
    service.cancel_login();
    show_step(app, STEP_CREDENTIALS);
}

/// The session is open: from here on this is the mailbox.
fn open_mailbox(service: &Arc<Service>, app: &Weak<App>) {
    let (service, app) = (service.clone(), app.clone());
    incoming::mailbox_open();
    keyboard::read_settings(&service, &app);
    tokio::spawn(async move {
        // A row is written once, so what the account chose for it has to be known first.
        // First: whether mail stands in conversations decides which counts are asked for.
        service.read_habits().await;
        let folder_count = if let (Ok(folders), Ok(counts), _) =
            futures::join!(service.folders(), service.counts(), service.time_settings())
        {
            view::folder_rows(&app, &folders, &counts, FIRST_FOLDER);
            folders.len()
        } else {
            view::failed(&app);
            0
        };
        view::loading_mail(&app);
        service.view_begin_paged(crate::list_style::paged());
        service.view_open(FIRST_FOLDER).await;
        crate::account_check::mailbox_opened(&service, &app, folder_count);
        let _ = app.upgrade_in_event_loop(|_| sync::request_check());
    });
}

/// The sidebar again, after folders or labels changed.
pub fn refresh_folders(service: &Arc<Service>, app: &Weak<App>) {
    refresh_folders_after(service, app, |_| {});
}

/// The folder open now stays the one lit, and `then` runs once the sidebar has been redrawn.
pub fn refresh_folders_after(service: &Arc<Service>, app: &Weak<App>, then: impl FnOnce(&App) + Send + 'static) {
    let (service, weak) = (service.clone(), app.clone());
    let active = crate::actions::main_window().map_or_else(|| FIRST_FOLDER.to_string(), |app| app.get_folder_id().to_string());
    tokio::spawn(async move {
        if let (Ok(folders), Ok(counts)) = (service.folders().await, service.counts().await) {
            view::folder_rows(&weak, &folders, &counts, &active);
        }
        let _ = weak.upgrade_in_event_loop(move |app| then(&app));
    });
}

pub fn start(service: &Arc<Service>, app: &Weak<App>) {
    let (service, weak) = (service.clone(), app.clone());
    tokio::spawn(async move {
        let step = service.reopen().await;
        go_on(&service, &weak, Ok(step));
    });
}

/// However the session ended, nothing of the account stays on screen.
pub fn session_ended(app: &Weak<App>, ending: Ending) {
    let _ = app.upgrade_in_event_loop(move |app| {
        incoming::mailbox_closed();
        preview::close();
        session::forget_unlock_key();
        composer::abandon();
        settings::abandon(&app);
        view::forget_mailbox(&app);
        show_step(&app, STEP_CREDENTIALS);
        app.global::<Auth>().set_session_ended(ending == Ending::Refused);
        drop(app.show());
    });
}

pub fn ask_to_sign_out(service: &Arc<Service>, app: &App) {
    if actions::ask_to_sign_out(app) {
        let service = service.clone();
        tokio::spawn(async move {
            if let Err(error) = service.sign_out().await {
                log::warn!("signing out on Proton's side failed: {}", error.kind());
            }
        });
    }
}

fn digits_of(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).take(CODE_LENGTH).collect()
}

pub fn connect(service: &Arc<Service>, app: &App) {
    let auth = app.global::<Auth>();
    let (signing, weak) = (service.clone(), app.as_weak());
    auth.on_sign_in(move || {
        let Some(app) = weak.upgrade() else { return };
        let auth = app.global::<Auth>();
        let username = auth.get_username().to_string();
        let password = Zeroizing::new(auth.get_password().to_string());
        let service = signing.clone();
        act(&signing, &app, async move { service.sign_in(&username, &password).await });
    });
    let (coding, weak) = (service.clone(), app.as_weak());
    let submit_code = move |code: String| {
        let Some(app) = weak.upgrade() else { return };
        let service = coding.clone();
        act(&coding, &app, async move { service.submit_totp(&code).await });
    };
    let submit = submit_code.clone();
    let weak = app.as_weak();
    auth.on_code_edited(move || {
        let Some(app) = weak.upgrade() else { return };
        let auth = app.global::<Auth>();
        let code = digits_of(&auth.get_code());
        show_code(&auth, &code);
        // A code works once, so a failed one is cleared rather than left to be sent again.
        if code.len() == CODE_LENGTH && !auth.get_busy() {
            submit(code);
        }
    });
    auth.on_recovery_sent(move |code| submit_code(code.trim().to_string()));
    let (opening, weak) = (service.clone(), app.as_weak());
    auth.on_mailbox_sent(move || {
        let Some(app) = weak.upgrade() else { return };
        let auth = app.global::<Auth>();
        let password = Zeroizing::new(auth.get_mailbox().to_string());
        let service = opening.clone();
        act(&opening, &app, async move { service.submit_mailbox_password(&password).await });
    });
    let (unlocking, weak) = (service.clone(), app.as_weak());
    auth.on_unlock(move || {
        let Some(app) = weak.upgrade() else { return };
        let auth = app.global::<Auth>();
        let password = Zeroizing::new(auth.get_lock_password().to_string());
        let service = unlocking.clone();
        act(&unlocking, &app, async move { service.unlock_app(&password).await });
    });
    let (touching, weak) = (service.clone(), app.as_weak());
    auth.on_touch_unlock(move || {
        let Some(app) = weak.upgrade() else { return };
        let reason = app.global::<Auth>().get_touch_reason().to_string();
        let service = touching.clone();
        act(&touching, &app, async move {
            let confirmed = tokio::task::spawn_blocking(move || session::touch_id::confirm(&reason)).await;
            if !confirmed.unwrap_or(false) {
                return Err(Error::Cancelled);
            }
            let key = session::unlock_key().ok_or(Error::WrongPassword)?;
            service.unlock_with_key(key).await
        });
    });
    let (forgetting, weak) = (service.clone(), app.as_weak());
    auth.on_forgot(move || {
        let Some(app) = weak.upgrade() else { return };
        if actions::ask_to_forget(&app) {
            forgetting.forget_locked_session();
        }
    });
    let weak = app.as_weak();
    auth.on_leave_tor(move || {
        let Some(app) = weak.upgrade() else { return };
        if actions::ask_to_leave_tor(&app) {
            log::info!("Tor left from the waiting screen");
            crate::tor_link::leave();
        }
    });
    connect_verification(service, app);
}

/// Proton's check of a person: back, in the browser, done, and the page kept over its place.
fn connect_verification(service: &Arc<Service>, app: &App) {
    let auth = app.global::<Auth>();
    let (leaving, weak) = (service.clone(), app.as_weak());
    auth.on_verify_back(move || {
        if let Some(app) = weak.upgrade() {
            back_to_credentials(&leaving, &app);
        }
    });
    let weak = app.as_weak();
    auth.on_verify_in_browser_requested(move || {
        let Some(app) = weak.upgrade() else { return };
        let auth = app.global::<Auth>();
        auth.set_verify_in_browser(true);
        view::open_link(&auth.get_verify_address());
    });
    let (returning, weak) = (service.clone(), app.as_weak());
    auth.on_verify_done(move || {
        if let Some(app) = weak.upgrade() {
            verification_ended(&returning, &app, None);
        }
    });
    let weak = app.as_weak();
    auth.on_verify_moved(move || {
        if let Some(app) = weak.upgrade() {
            verify::place(&app);
        }
    });
    auth.on_repository_requested(|| view::open_link(proton_core::REPOSITORY));
    crate::account_panel::connect(service, app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_digits_and_no_more_of_them_than_it_needs() {
        assert_eq!(digits_of("12 34a5678"), "123456");
        assert_eq!(digits_of("abc"), "");
    }

    #[test]
    fn some_failures_say_nothing() {
        assert_eq!(error_of(&Error::Cancelled).0, 0);
        assert_eq!(error_of(&Error::SessionEnded).0, 0);
        assert_eq!(error_of(&Error::TooManyAttempts { seconds: 5 }).0, 0);
    }

    #[test]
    fn proton_s_own_words_are_shown_with_their_code() {
        let error = Error::Api { status: 422, code: 8002, message: "Incorrect login".to_string() };
        assert_eq!(error_of(&error), (ERROR_SAID, "Incorrect login (code 8002, HTTP 422)".to_string()));
    }

    #[test]
    fn a_silent_server_gets_words_from_the_app() {
        let error = Error::Api { status: 503, code: 0, message: String::new() };
        assert_eq!(error_of(&error).0, ERROR_UNAVAILABLE);
    }

    #[test]
    fn connection_failures_are_told_apart() {
        assert_eq!(error_of(&Error::Network("x".to_string())).0, ERROR_NETWORK);
        assert_eq!(error_of(&Error::Untrusted).0, ERROR_UNTRUSTED);
        assert_eq!(error_of(&Error::WrongPassword).0, ERROR_WRONG_PASSWORD);
    }
}
