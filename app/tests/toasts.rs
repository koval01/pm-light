//! A toast is a number on the Rust side and words in `app.slint`. Every number is named once, in
//! `toasts.rs`, the window has words for each, and no call passes a bare number.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "a test that cannot read its files must fail")]

use std::{collections::HashMap, fs, path::PathBuf};

fn app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn named() -> HashMap<i32, String> {
    let text = fs::read_to_string(app().join("src/toasts.rs")).expect("the toasts");
    let mut found = HashMap::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("pub const ") else { continue };
        let (name, value) = rest.split_once(": i32 = ").expect("a toast is a number");
        let number: i32 = value.trim_end_matches(';').parse().expect("a number");
        assert!(found.insert(number, name.to_string()).is_none(), "{number} is named twice");
    }
    found
}

fn numbers_after(text: &str, start: &str) -> Vec<i32> {
    text.match_indices(start)
        .filter_map(|(at, _)| {
            let rest = text.get(at + start.len()..)?;
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}

#[test]
fn every_toast_has_words() {
    let named = named();
    assert!(named.len() > 10, "the toasts did not load");
    let window = fs::read_to_string(app().join("ui/app.slint")).expect("the window");
    let worded = numbers_after(&window, "root.toast == ");
    for (number, name) in &named {
        assert!(worded.contains(number), "{name} ({number}) has no words in app.slint");
    }
    for number in worded.iter().chain(&numbers_after(&window, "root.toast = ")) {
        assert!(*number == 0 || named.contains_key(number), "app.slint knows {number}, toasts.rs does not");
    }
}

#[test]
fn no_call_passes_a_bare_number() {
    for entry in fs::read_dir(app().join("src")).expect("the sources").flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|end| end != "rs") || path.ends_with("toasts.rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("a source");
        for start in ["toast(", "set_toast("] {
            let bare = numbers_after(&text, start);
            assert!(bare.is_empty(), "{} passes {bare:?} to {start}", path.display());
        }
        assert!(!text.contains("const TOAST_"), "{} names a toast of its own", path.display());
    }
}
