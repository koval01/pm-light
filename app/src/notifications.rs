//! Says how many, never who or what: a notification is visible on the lock screen.

use crate::{App, Words, preferences};
use slint::{ComponentHandle, winit_030::WinitWindowAccessor};

#[cfg(any(target_os = "macos", target_os = "windows"))]
use crate::guard::IDENTIFIER;

/// The system said no to notifications and badges: the count on the Dock and the notifications are not seen until
/// the person allows them in the system's settings, which the app's settings then point to.
static REFUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn refused_by_the_system() -> bool {
    REFUSED.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
pub fn prepare() {
    let _ = notify_rust::set_application(IDENTIFIER);
    ask_for_badges();
}

/// The system shows an app's Dock badge only once the app has asked for notifications with badges: before, the
/// number was set and never seen. A program outside a bundle has no place among the system's apps, and asking from
/// one ends it, so only the bundle asks.
/// Whether the program runs from an app bundle, the only place the system's notification centre takes it from.
#[cfg(any(target_os = "macos", test))]
fn is_bundled(path: &str) -> bool {
    std::path::Path::new(path).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
}

#[cfg(target_os = "macos")]
fn ask_for_badges() {
    use objc2_foundation::{NSBundle, NSError};
    use objc2_user_notifications::{UNAuthorizationOptions, UNUserNotificationCenter};

    // A program outside a bundle can still carry an identifier, from its signature: what counts is the folder it
    // runs from. The bare program in target/native ended at its start on this.
    if !is_bundled(&NSBundle::mainBundle().bundlePath().to_string()) {
        return;
    }
    let answered = block2::RcBlock::new(|granted: objc2::runtime::Bool, _error: *mut NSError| {
        log::info!("notifications and badges: {}", if granted.as_bool() { "allowed" } else { "not allowed" });
        REFUSED.store(!granted.as_bool(), std::sync::atomic::Ordering::Relaxed);
    });
    let options = UNAuthorizationOptions::Badge | UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound;
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(options, &answered);
}

#[cfg(target_os = "windows")]
pub fn prepare() {
    windows::register();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const fn prepare() {}

/// Windows shows a notification under the name an app id is registered with. With none, it comes
/// from PowerShell, or does not show at all. An installer would register it; until there is one,
/// the app does, for this user only, under `HKCU\Software\Classes\AppUserModelId`.
#[cfg(target_os = "windows")]
mod windows {
    use super::IDENTIFIER;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
            RegSetValueExW,
        },
        UI::Shell::SetCurrentProcessExplicitAppUserModelID,
    };

    const NAME: &str = "Proton Mail Light";
    const DONE: u32 = 0;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn register() {
        let id = wide(IDENTIFIER);
        // Safety: the string is terminated and outlives the call, which copies it.
        if unsafe { SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) } < 0 {
            log::warn!("the app id was not taken");
        }
        let path = wide(&format!("Software\\Classes\\AppUserModelId\\{IDENTIFIER}"));
        let mut key: HKEY = null_mut();
        // Safety: every pointer is to a terminated string or a local that outlives the call.
        let opened = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                null(),
                &raw mut key,
                null_mut(),
            )
        };
        if opened != DONE {
            log::warn!("the app id could not be registered: {opened}");
            return;
        }
        let (field, value) = (wide("DisplayName"), wide(NAME));
        let bytes = u32::try_from(value.len() * 2).unwrap_or(0);
        // Safety: the value is a terminated UTF-16 string of exactly `bytes` bytes, and the key is open.
        let set = unsafe { RegSetValueExW(key, field.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), bytes) };
        if set != DONE {
            log::warn!("the app's name could not be registered: {set}");
        }
        // Safety: the key was opened above and is closed once.
        unsafe { RegCloseKey(key) };
    }
}

fn focused(app: &App) -> bool {
    app.window().with_winit_window(slint::winit_030::winit::window::Window::has_focus).unwrap_or(false)
}

pub fn announce(app: &App, count: u64) {
    log::debug!("new mail: {count}");
    if focused(app) || !preferences::get().notifications {
        return;
    }
    let words = app.global::<Words>();
    let title = words.get_notify_title().to_string();
    let body = words.invoke_notify_body(count.to_string().into()).to_string();
    // Off the window's thread: on some systems this waits for the notification to be delivered.
    tokio::task::spawn_blocking(move || {
        let mut notification = notify_rust::Notification::new();
        notification.summary(&title).body(&body);
        #[cfg(target_os = "windows")]
        notification.app_id(IDENTIFIER);
        if notification.show().is_err() {
            log::warn!("could not show a notification");
        }
    });
}

#[cfg(test)]
mod tests;
