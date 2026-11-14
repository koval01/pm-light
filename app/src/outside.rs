//! What leaves the app for good: a link for the browser, and the system's own settings for notifications. Both are
//! refused in a check, which no one would come back from (`checks`).

use crate::actions;

/// The system's own settings for this app's notifications, where only the person can allow them.
pub fn open_notification_settings() {
    #[cfg(target_os = "macos")]
    let page = "x-apple.systempreferences:com.apple.Notifications-Settings.extension";
    #[cfg(target_os = "windows")]
    let page = "ms-settings:notifications";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let page = "";
    if page.is_empty() {
        return;
    }
    if crate::checks::refuses("the system's notification settings") {
        return;
    }
    log::info!("opening the system's notification settings");
    if let Err(error) = open::that_detached(page) {
        log::warn!("the system's notification settings did not open: {:?}", error.kind());
    }
}

pub fn open_link(url: &str) {
    let Some(parsed) = proton_core::safety::openable(url) else {
        log::info!("a link was not opened: not a web or mail address");
        return;
    };
    // The browser is not the app, and does not go through Tor: the person says so first, each time.
    if crate::tor_link::chosen() && matches!(parsed.scheme(), "http" | "https") {
        let Some(app) = actions::main_window() else { return };
        let say = |key: &str| crate::settings::say(&app, key);
        if !actions::confirmed(
            &say("links.outsideTorTitle"),
            &say("links.outsideTorText"),
            &say("links.open"),
            &say("common.cancel"),
        ) {
            log::info!("a link stayed unopened, inside Tor");
            return;
        }
    }
    if crate::checks::refuses("a link for the browser") {
        return;
    }
    log::info!("opening a {} link", parsed.scheme());
    if let Err(error) = open::that_detached(parsed.as_str()) {
        log::warn!("the system did not open a link: {:?}", error.kind());
    }
}
