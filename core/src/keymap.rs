//! Keyboard shortcuts: which keys mean what, and chords such as `g i`. A key is named by what it
//! is on a Latin layout, so that the same place on the keyboard does the same on a Cyrillic one.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Next,
    Previous,
    Open,
    Close,
    ScrollDown,
    ScrollUp,
    Search,
    Compose,
    Reply,
    ReplyAll,
    Forward,
    Archive,
    Trash,
    Spam,
    Star,
    Unread,
    Move,
    Label,
    Images,
    ToggleCheck,
    CheckAll,
    Headers,
    Undo,
    Help,
    Go(&'static str),
    Contacts,
}

const CHORD: Duration = Duration::from_millis(1200);

/// A chord is two keys separated by a space, as in `main`.
const KEYMAP: [(&str, Action); 35] = [
    ("j", Action::Next),
    ("ArrowDown", Action::Next),
    ("k", Action::Previous),
    ("ArrowUp", Action::Previous),
    ("Enter", Action::Open),
    ("Escape", Action::Close),
    ("Space", Action::ScrollDown),
    ("Shift+Space", Action::ScrollUp),
    ("/", Action::Search),
    ("c", Action::Compose),
    ("r", Action::Reply),
    ("a", Action::ReplyAll),
    ("f", Action::Forward),
    ("e", Action::Archive),
    ("#", Action::Trash),
    ("!", Action::Spam),
    ("s", Action::Star),
    ("u", Action::Unread),
    ("m", Action::Move),
    ("l", Action::Label),
    ("i", Action::Images),
    ("x", Action::ToggleCheck),
    ("*", Action::CheckAll),
    ("h", Action::Headers),
    ("z", Action::Undo),
    ("?", Action::Help),
    ("g i", Action::Go("inbox")),
    ("g s", Action::Go("starred")),
    ("g d", Action::Go("drafts")),
    ("g t", Action::Go("sent")),
    ("g a", Action::Go("archive")),
    ("g !", Action::Go("spam")),
    ("g #", Action::Go("trash")),
    ("g m", Action::Go("all")),
    ("g c", Action::Contacts),
];

const CYRILLIC: &str = "\u{439}\u{446}\u{443}\u{43a}\u{435}\u{43d}\u{433}\u{448}\u{449}\u{437}\u{444}\u{44b}\u{432}\u{430}\u{43f}\u{440}\u{43e}\u{43b}\u{434}\u{44f}\u{447}\u{441}\u{43c}\u{438}\u{442}\u{44c}";
const LATIN: &str = "qwertyuiopasdfghjklzxcvbnm";

/// The Latin key in the same place as a key of the Russian layout, and any other key as it is.
#[must_use]
pub fn latin_key(key: &str) -> String {
    let mut chars = key.chars();
    let (Some(only), None) = (chars.next(), chars.next()) else { return key.to_string() };
    if only == '.' {
        return "/".to_string();
    }
    only.to_lowercase().next().and_then(|lower| CYRILLIC.chars().position(|c| c == lower)).map_or_else(|| key.to_string(), |at| LATIN.chars().nth(at).map_or_else(|| key.to_string(), String::from))
}

/// One row of the keyboard help: an action, with every key that does it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub action: Action,
    pub keys: String,
    pub chord: bool,
}

fn key_label(key: &str) -> &str {
    match key {
        "ArrowDown" => "↓",
        "ArrowUp" => "↑",
        "Escape" => "Esc",
        "Space" => "␣",
        "Shift+Space" => "⇧ ␣",
        other => other,
    }
}

#[must_use]
pub fn shortcuts() -> Vec<Shortcut> {
    let mut rows: Vec<Shortcut> = Vec::new();
    for (keys, action) in KEYMAP {
        let chord = keys.contains(' ');
        match rows.iter_mut().find(|row| row.action == action) {
            Some(row) => {
                row.keys.push_str("  ");
                row.keys.push_str(key_label(keys));
            }
            None => rows.push(Shortcut { action, keys: key_label(keys).to_string(), chord }),
        }
    }
    rows
}

fn starts_a_chord(key: &str) -> bool {
    KEYMAP.iter().any(|(keys, _)| keys.split_once(' ').is_some_and(|(first, _)| first == key))
}

fn action_of(keys: &str) -> Option<Action> {
    KEYMAP.iter().find(|(known, _)| *known == keys).map(|(_, action)| *action)
}

/// Fed one key at a time, it answers with an action once a binding is complete.
#[derive(Default)]
pub struct KeyReader {
    pending: Option<(String, Instant)>,
}

impl KeyReader {
    pub fn read(&mut self, key: &str) -> Option<Action> {
        self.read_at(key, Instant::now())
    }

    /// A second key that makes no chord only ends it.
    fn read_at(&mut self, key: &str, now: Instant) -> Option<Action> {
        let key = latin_key(key);
        let chord = self.pending.take().filter(|(_, at)| now.duration_since(*at) <= CHORD);
        if let Some((first, _)) = chord {
            return action_of(&format!("{first} {key}"));
        }
        if starts_a_chord(&key) {
            self.pending = Some((key, now));
            return None;
        }
        action_of(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_keys_mean_their_action() {
        let mut reader = KeyReader::default();
        assert_eq!(reader.read("j"), Some(Action::Next));
        assert_eq!(reader.read("ArrowUp"), Some(Action::Previous));
        assert_eq!(reader.read("#"), Some(Action::Trash));
        assert_eq!(reader.read("Shift+Space"), Some(Action::ScrollUp));
        assert_eq!(reader.read("q"), None);
    }

    #[test]
    fn a_chord_needs_both_keys_in_time() {
        let mut reader = KeyReader::default();
        let start = Instant::now();
        assert_eq!(reader.read_at("g", start), None);
        assert_eq!(reader.read_at("i", start + Duration::from_millis(500)), Some(Action::Go("inbox")));
        assert_eq!(reader.read_at("g", start), None);
        assert_eq!(reader.read_at("i", start + CHORD + Duration::from_millis(1)), Some(Action::Images));
    }

    #[test]
    fn a_wrong_second_key_only_ends_the_chord() {
        let mut reader = KeyReader::default();
        assert_eq!(reader.read("g"), None);
        assert_eq!(reader.read("q"), None);
        assert_eq!(reader.read("j"), Some(Action::Next));
    }

    #[test]
    fn a_cyrillic_layout_reaches_the_same_keys() {
        assert_eq!(latin_key("\u{441}"), "c");
        assert_eq!(latin_key("\u{43e}"), "j");
        assert_eq!(latin_key("\u{43b}"), "k");
        assert_eq!(latin_key("\u{421}"), "c");
        assert_eq!(latin_key("."), "/");
        let mut reader = KeyReader::default();
        assert_eq!(reader.read("\u{441}"), Some(Action::Compose));
        assert_eq!(reader.read("\u{43f}"), None);
        assert_eq!(reader.read("\u{448}"), Some(Action::Go("inbox")));
    }

    #[test]
    fn other_keys_pass_as_they_are() {
        assert_eq!(latin_key("ArrowDown"), "ArrowDown");
        assert_eq!(latin_key("#"), "#");
        assert_eq!(latin_key(""), "");
    }

    #[test]
    fn the_help_has_one_row_for_each_action() {
        let rows = shortcuts();
        let next = rows.iter().find(|row| row.action == Action::Next);
        assert_eq!(next.map(|row| row.keys.as_str()), Some("j  ↓"));
        assert!(rows.iter().any(|row| row.chord && row.keys == "g i"));
        assert!(rows.iter().all(|row| row.chord == row.keys.starts_with("g ")));
    }

    #[test]
    fn the_two_alphabets_line_up() {
        assert_eq!(CYRILLIC.chars().count(), LATIN.chars().count());
    }
}
