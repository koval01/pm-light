//! What belongs to this computer and not to the account. A field a file does not have takes its default, so a file
//! written by an older build keeps what it has.

use parking_lot::Mutex;
use proton_core::print::PrintOptions;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::LazyLock};

const FILE: &str = "preferences.json";
const DEFAULT_AUTO_LOCK_MINUTES: u64 = 5;
const DEFAULT_ZOOM: u32 = 100;

#[expect(clippy::struct_excessive_bools, reason = "each is one switch of the settings")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub window: Option<crate::window_place::WindowPlace>,
    pub auto_lock_minutes: u64,
    pub notifications: bool,
    pub zoom: u32,
    pub close_to_tray: bool,
    /// Keeps read mail on disk, encrypted, for the next start.
    pub mail_on_disk: bool,
    /// An IANA name. Empty means the system's zone.
    pub time_zone: String,
    pub print: PrintOptions,
    /// 0 follows the system, 1 is light, 2 is dark.
    pub theme: u8,
    pub palette: u8,
    pub compact: bool,
    pub rows: bool,
    pub mail_text: u8,
    /// Empty means the system's language.
    pub language: String,
    /// Whether everything goes through Tor. `None` until the person has been asked, at the first start.
    pub tor: Option<TorChoice>,
    /// The tour of the window waits to be shown. Only a first start sets it: someone who used the app before has a
    /// file without it, which reads as false.
    pub tour_pending: bool,
    /// Addresses, IP addresses and the like stay covered until pointed at, for a screen others can see.
    pub hide_private: bool,
    /// An icon in the menu bar or the tray. On macOS the Dock is the way back to a closed window, and the icon is
    /// left out unless asked for; elsewhere it is the only way back, and always there.
    pub menu_bar_icon: bool,
    /// Pictures of people from Gravatar, for senders with no logo. Off unless the person turns it on, after being
    /// told that Gravatar learns, in coded form, the addresses of the people who write to them.
    pub gravatar: bool,
    /// A folder a page at a time, or one list that grows as it is scrolled. `None` until the person has been asked,
    /// once, after the mailbox first opened; pages until then.
    pub list_pages: Option<bool>,
    /// Told that a picture of a mail leaves Proton's encryption, before the first was made.
    pub picture_warned: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TorChoice {
    pub on: bool,
    pub route: proton_core::tor::Route,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            window: None,
            auto_lock_minutes: DEFAULT_AUTO_LOCK_MINUTES,
            notifications: true,
            zoom: DEFAULT_ZOOM,
            close_to_tray: true,
            mail_on_disk: true,
            time_zone: String::new(),
            print: PrintOptions::default(),
            theme: 0,
            palette: 0,
            compact: false,
            rows: false,
            mail_text: 1,
            language: String::new(),
            tor: None,
            tour_pending: false,
            hide_private: true,
            menu_bar_icon: !cfg!(target_os = "macos"),
            gravatar: false,
            list_pages: None,
            picture_warned: false,
        }
    }
}

static CURRENT: LazyLock<Mutex<Preferences>> = LazyLock::new(|| Mutex::new(load()));

fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(crate::guard::IDENTIFIER).join(FILE))
}

/// A file that does not read as preferences must not stop the app from starting.
fn parse(text: &str) -> Preferences {
    serde_json::from_str(text).unwrap_or_default()
}

fn load() -> Preferences {
    path().and_then(|path| fs::read_to_string(path).ok()).map_or_else(first_start, |text| parse(&text))
}

/// No file yet: the app has never run here, and the tour is offered once the mailbox opens.
fn first_start() -> Preferences {
    Preferences { tour_pending: true, ..Preferences::default() }
}

fn save(preferences: &Preferences) {
    let Some(path) = path() else { return };
    let Ok(text) = serde_json::to_string_pretty(preferences) else { return };
    if let Some(folder) = path.parent() {
        let _ = fs::create_dir_all(folder);
    }
    if let Err(error) = fs::write(&path, text) {
        log::warn!("could not save preferences: {}", error.kind());
    }
}

pub fn get() -> Preferences {
    CURRENT.lock().clone()
}

pub fn change(edit: impl FnOnce(&mut Preferences)) {
    let mut preferences = CURRENT.lock();
    edit(&mut preferences);
    save(&preferences);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tour_waits_only_after_a_first_start() {
        assert!(first_start().tour_pending);
        // A file from a version without the tour: its owner has used the app, and is not shown it.
        assert!(!parse(r#"{"notifications": true, "zoom": 100}"#).tour_pending);
        assert!(!parse("not json").tour_pending);
        assert!(parse(r#"{"tourPending": true}"#).tour_pending);
    }

    #[test]
    fn the_menu_bar_icon_is_left_out_only_on_macos() {
        assert_eq!(Preferences::default().menu_bar_icon, !cfg!(target_os = "macos"));
        assert_eq!(parse(r#"{"notifications": true}"#).menu_bar_icon, !cfg!(target_os = "macos"));
        assert!(parse(r#"{"menuBarIcon": true}"#).menu_bar_icon);
    }

    /// Gravatar tells a third party about the people one writes with: never on by itself.
    #[test]
    fn gravatar_is_off_unless_turned_on() {
        assert!(!Preferences::default().gravatar);
        assert!(!first_start().gravatar);
        assert!(!parse(r#"{"notifications": true}"#).gravatar);
        assert!(parse(r#"{"gravatar": true}"#).gravatar);
    }

    /// Covering private details is the default, and a file from before the setting existed keeps it.
    #[test]
    fn private_details_stay_covered_unless_asked() {
        assert!(Preferences::default().hide_private);
        assert!(first_start().hide_private);
        assert!(parse(r#"{"notifications": true}"#).hide_private);
        assert!(!parse(r#"{"hidePrivate": false}"#).hide_private);
    }

    #[test]
    fn a_damaged_file_gives_the_defaults() {
        assert_eq!(parse("not json"), Preferences::default());
        assert_eq!(parse("[1,2]"), Preferences::default());
    }

    #[test]
    fn a_file_of_an_older_build_keeps_what_it_has() {
        let preferences = parse(r#"{"autoLockMinutes": 15}"#);
        assert_eq!(preferences.auto_lock_minutes, 15);
        assert_eq!(preferences.mail_text, 1);
        assert!(preferences.notifications);
    }

    #[test]
    fn preferences_survive_a_round_trip() {
        let preferences = Preferences { palette: 3, language: "de".into(), ..Preferences::default() };
        assert_eq!(serde_json::to_string(&preferences).ok().map(|text| parse(&text)), Some(preferences));
    }
}
