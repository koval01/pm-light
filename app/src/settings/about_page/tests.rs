use super::{COMPONENTS, Component, LICENSE, NOTICE, NOTICES, components, matching};
use proton_core::REPOSITORY;

#[test]
fn the_license_is_the_agpl_from_the_repository() {
    assert!(LICENSE.trim_start().starts_with("GNU AFFERO GENERAL PUBLIC LICENSE"));
    assert!(LICENSE.contains("Version 3, 19 November 2007"));
    assert!(LICENSE.contains("13. Remote Network Interaction; Use with the GNU General Public License."));
}

/// The terms that keep the author and the repository: the About page and the file must say the same.
#[test]
fn the_additional_terms_keep_the_author_and_the_repository() {
    assert!(NOTICE.contains("Yaroslav Koval"));
    assert!(NOTICE.contains(REPOSITORY));
    assert!(NOTICE.contains("section 7"));
    assert!(NOTICE.contains("not affiliated with"));
    assert!(super::COPYRIGHT.contains("Yaroslav Koval"));
    assert_eq!(REPOSITORY, "https://github.com/koval01/pm-light");
}

#[test]
fn the_components_of_this_system_are_listed() {
    let all: Vec<_> = components().collect();
    assert!(all.len() > 300, "only {} components", all.len());
    assert_eq!(all.len(), COMPONENTS.lines().count(), "a line of the list did not read");
    for wanted in ["slint", "arti-client", "proton-crypto", "reqwest", "rustls"] {
        assert!(all.iter().any(|component| component.name == wanted), "{wanted} is missing");
    }
    for component in &all {
        assert!(!component.version.is_empty() && !component.license.is_empty(), "{component:?}");
    }
    let system = if cfg!(target_os = "macos") {
        'm'
    } else if cfg!(target_os = "windows") {
        'w'
    } else {
        'l'
    };
    assert!(COMPONENTS.lines().all(|line| line.rsplit('\t').next().is_some_and(|systems| systems.contains(system))));
}

#[test]
fn the_license_texts_come_with_the_program() {
    let texts = String::from_utf8(liblzma::decode_all(NOTICES).expect("the texts decompress")).expect("text");
    assert!(texts.contains("Apache License"));
    assert!(texts.contains("Permission is hereby granted"));
    for component in components().take(50) {
        let label = format!("{} {}", component.name, component.version);
        assert!(texts.contains(&label), "{label} has no text");
    }
}

#[test]
fn a_search_finds_by_name_or_license_and_ignores_case() {
    let all = || {
        [
            Component { name: "slint", version: "1", license: "GPL-3.0-only" },
            Component { name: "arti-client", version: "2", license: "MIT OR Apache-2.0" },
        ]
        .into_iter()
    };
    assert_eq!(matching("", all()).len(), 2);
    assert_eq!(matching("  SLINT ", all()).len(), 1);
    assert_eq!(matching("apache", all()).first().map(|found| found.name), Some("arti-client"));
    assert!(matching("nothing like it", all()).is_empty());
}
