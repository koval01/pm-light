//! The shape of the code, which no compiler checks: tests in files of their own, a word on what each file is for, files
//! of a size a person can hold in mind, and `unsafe` only where it has been read. A rule broken fails here with the
//! file that broke it.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Past this a file is split before more is added to it.
const MOST_LINES: usize = 500;

/// Files that grew past `MOST_LINES` before the rule, each at most as long as it was then: they may only shrink, and
/// leave this list once they are short enough.
const STILL_LONG: &[(&str, usize)] = &[
    ("app/src/view.rs", 1428),
    ("app/src/composer.rs", 982),
    ("core/src/client.rs", 710),
    ("core/src/html.rs", 634),
    ("core/src/search.rs", 578),
    ("core/src/model.rs", 572),
    ("app/src/settings/mail_pages.rs", 560),
    ("app/src/mail_view.rs", 529),
];

/// The files where `unsafe` has been read and each block says why it is sound. The core has none, and forbids it.
const UNSAFE_READ: &[&str] = &[
    "app/src/incoming.rs",
    "app/src/look.rs",
    "app/src/mail_picture/macos.rs",
    "app/src/mail_picture/windows.rs",
    "app/src/mail_view.rs",
    "app/src/memory.rs",
    "app/src/menu.rs",
    "app/src/modal.rs",
    "app/src/notifications.rs",
    "app/src/print_view.rs",
    "app/src/protection.rs",
    "app/src/session.rs",
    "app/src/tray.rs",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn rust_files(folder: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, found)?;
        } else if path.extension().is_some_and(|end| end == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

/// Every source file of both crates, by its path from the repository's root, with its text.
fn sources() -> io::Result<Vec<(String, String)>> {
    let root = root();
    let mut found = Vec::new();
    for crate_source in ["app/src", "core/src"] {
        rust_files(&root.join(crate_source), &mut found)?;
    }
    let mut files = Vec::new();
    for path in found {
        let name = path.strip_prefix(&root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
        files.push((name, fs::read_to_string(&path)?));
    }
    files.sort();
    Ok(files)
}

fn is_test_file(name: &str) -> bool {
    name.ends_with("/tests.rs") || name.ends_with("_tests.rs")
}

#[test]
fn tests_live_in_files_of_their_own() {
    let inline: Vec<String> = sources()
        .expect("the sources")
        .into_iter()
        .filter(|(_, text)| text.lines().any(|line| line.trim_start().starts_with("mod tests {")))
        .map(|(name, _)| name)
        .collect();
    assert!(inline.is_empty(), "tests belong in name/tests.rs beside name.rs: {inline:?}");
}

#[test]
fn every_file_says_what_it_is_for() {
    let silent: Vec<String> = sources()
        .expect("the sources")
        .into_iter()
        .filter(|(name, text)| !is_test_file(name) && !text.starts_with("//!"))
        .map(|(name, _)| name)
        .collect();
    assert!(silent.is_empty(), "a file begins with a `//!` line on what it is for: {silent:?}");
}

#[test]
fn files_stay_short_enough_to_read() {
    let mut too_long = Vec::new();
    for (name, text) in sources().expect("the sources").into_iter().filter(|(name, _)| !is_test_file(name)) {
        let lines = text.lines().count();
        let most = STILL_LONG.iter().find(|(long, _)| *long == name).map_or(MOST_LINES, |(_, then)| *then);
        if lines > most {
            too_long.push(format!("{name}: {lines} lines, at most {most}"));
        }
    }
    assert!(too_long.is_empty(), "split these before adding to them: {too_long:?}");
}

#[test]
fn the_list_of_long_files_holds_only_long_files_that_exist() {
    let files = sources().expect("the sources");
    for (name, then) in STILL_LONG {
        let lines = files.iter().find(|(file, _)| file == name).map(|(_, text)| text.lines().count());
        let lines = lines.unwrap_or_else(|| panic!("{name} is gone: take it off the list"));
        assert!(lines > MOST_LINES, "{name} is short enough now: take it off the list");
        assert!(*then > MOST_LINES, "{name} was never long");
    }
}

#[test]
fn unsafe_stays_where_it_was_read() {
    let unread: Vec<String> = sources()
        .expect("the sources")
        .into_iter()
        .filter(|(name, text)| !is_test_file(name) && !UNSAFE_READ.contains(&name.as_str()) && text.contains("unsafe "))
        .map(|(name, _)| name)
        .collect();
    assert!(unread.is_empty(), "unsafe in a file not yet read for it; read it, then list it here: {unread:?}");
    for name in UNSAFE_READ {
        let text = sources().expect("the sources").into_iter().find(|(file, _)| file == name).map(|(_, text)| text);
        assert!(
            text.is_some_and(|text| text.contains("unsafe ")),
            "{name} has no unsafe any more: take it off the list"
        );
    }
}

#[test]
fn the_core_forbids_unsafe() {
    let core = fs::read_to_string(root().join("core/src/lib.rs")).expect("the core");
    assert!(core.contains("#![cfg_attr(not(test), forbid(unsafe_code))]"), "the core forbids unsafe code");
}
