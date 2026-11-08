//! Everything written into the project is English. Other languages live in `app/lang` alone.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "a test that cannot read its files must fail")]

use std::{
    fs,
    path::{Path, PathBuf},
};

const ROOTS: [&str; 12] = [
    "README.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SUPPORT.md",
    "THIRD-PARTY-NOTICES.md",
    "Cargo.toml",
    "docs",
    "scripts",
    ".github",
    "core/src",
    "app/src",
    "app/ui",
];
const TEXT: [&str; 7] = ["md", "rs", "sh", "toml", "slint", "yml", "json"];
const GERMAN: [char; 7] = ['\u{c4}', '\u{d6}', '\u{dc}', '\u{df}', '\u{e4}', '\u{f6}', '\u{fc}'];

fn is_foreign(c: char) -> bool {
    ('\u{400}'..='\u{52f}').contains(&c) || GERMAN.contains(&c)
}

fn files(path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() {
        return vec![path.to_path_buf()];
    }
    fs::read_dir(path).expect("a folder of the project").flatten().flat_map(|entry| files(&entry.path())).collect()
}

#[test]
fn other_languages_stay_in_the_language_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the project");
    let checked: Vec<PathBuf> = ROOTS.iter().flat_map(|name| files(&root.join(name))).collect();
    assert!(checked.len() > 100, "the project was not found");
    let offenders: Vec<&PathBuf> = checked
        .iter()
        .filter(|path| path.extension().is_some_and(|end| TEXT.iter().any(|known| end == *known)))
        .filter(|path| fs::read_to_string(path).is_ok_and(|text| text.chars().any(is_foreign)))
        .collect();
    assert!(offenders.is_empty(), "{offenders:#?}");
}
