//! The list of what the program is built from stays true to `Cargo.lock`: a crate added, removed or moved to another
//! version fails here until `scripts/components.py` has been run, so that the About page and the license texts
//! that come with the program never describe an older program.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "a test that cannot read its files must fail")]

use std::{collections::BTreeSet, fs, path::Path};

const OWN: [&str; 2] = ["proton-core", "proton-mail-light"];

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the project")
}

/// Every crate of the lock file, as `name version`.
fn locked(lock: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for package in lock.split("[[package]]").skip(1) {
        let field = |key: &str| {
            package
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key} = \""))?.strip_suffix('"').map(str::to_string))
        };
        if let (Some(name), Some(version)) = (field("name"), field("version"))
            && !OWN.contains(&name.as_str())
        {
            found.insert(format!("{name} {version}"));
        }
    }
    found
}

/// The crates the two files of `components/` account for, and the problems with them.
fn accounted(list: &str, unshipped: &str) -> (BTreeSet<String>, Vec<String>) {
    let mut found = BTreeSet::new();
    let mut problems = Vec::new();
    for line in list.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [name, version, license, _, systems] = fields.as_slice() else {
            problems.push(format!("a line of list.tsv is not five fields: {line}"));
            continue;
        };
        if license.is_empty() || systems.is_empty() || systems.chars().any(|system| !"mwl".contains(system)) {
            problems.push(format!("{name} {version} has no license or no system"));
        }
        if !found.insert(format!("{name} {version}")) {
            problems.push(format!("{name} {version} is listed twice"));
        }
    }
    for line in unshipped.lines().filter(|line| !line.is_empty()) {
        if !found.insert(line.to_string()) {
            problems.push(format!("{line} is both shipped and not"));
        }
    }
    (found, problems)
}

fn differences(locked: &BTreeSet<String>, accounted: &BTreeSet<String>) -> Vec<String> {
    let missing = locked.difference(accounted).map(|crate_| format!("{crate_} is not in components/"));
    let stale = accounted.difference(locked).map(|crate_| format!("{crate_} is no longer in Cargo.lock"));
    missing.chain(stale).collect()
}

#[test]
fn the_components_are_those_of_cargo_lock() {
    let read = |path: &str| fs::read_to_string(root().join(path)).expect(path);
    let lock = locked(&read("Cargo.lock"));
    assert!(lock.len() > 500, "the lock file did not read");
    let (found, mut problems) = accounted(&read("app/components/list.tsv"), &read("app/components/unshipped.txt"));
    problems.extend(differences(&lock, &found));
    assert!(problems.is_empty(), "run scripts/components.py:\n{problems:#?}");
}

#[test]
fn every_component_has_its_texts() {
    let read = |path: &str| fs::read_to_string(root().join(path)).expect(path);
    let notices = read("app/components/notices.txt");
    let carriers: BTreeSet<&str> = notices
        .split("\n== ")
        .filter_map(|block| block.trim_start_matches("== ").lines().next())
        .flat_map(|line| line.split(", "))
        .collect();
    let list = read("app/components/list.tsv");
    let missing: Vec<_> = list
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let label = format!("{} {}", fields.next()?, fields.next()?);
            (!carriers.contains(label.as_str())).then_some(label)
        })
        .collect();
    assert!(missing.is_empty(), "run scripts/components.py: {missing:?}");
}

#[test]
fn the_check_sees_a_change_of_version_a_new_crate_and_a_gone_one() {
    let lock = "[[package]]\nname = \"a\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"b\"\nversion = \"2.0.0\"\n\n[[package]]\nname = \"proton-core\"\nversion = \"0.1.0\"\n";
    let locked = locked(lock);
    assert_eq!(locked.len(), 2);
    let (same, problems) = accounted("a\t1.0.0\tMIT\t\tm\n", "b 2.0.0\n");
    assert!(problems.is_empty() && differences(&locked, &same).is_empty());
    let (newer, _) = accounted("a\t1.0.1\tMIT\t\tm\n", "b 2.0.0\n");
    assert_eq!(differences(&locked, &newer).len(), 2);
    let (fewer, _) = accounted("", "b 2.0.0\n");
    assert_eq!(differences(&locked, &fewer).len(), 1);
    let (_, bad) = accounted("a\t1.0.0\t\t\t\n", "");
    assert_eq!(bad.len(), 1);
}
