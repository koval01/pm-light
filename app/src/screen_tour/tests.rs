use super::*;

#[test]
fn the_tour_shows_every_mail_folder_letter_and_settings_page() {
    let all = steps(9);
    assert_eq!(all.first(), Some(&Step::Mail(0)), "the first step is the window as the check left it");
    for index in 0..9 {
        assert!(all.contains(&Step::Mail(index)));
    }
    assert!(!all.contains(&Step::Mail(9)));
    for folder in FOLDERS {
        assert!(all.contains(&Step::Folder(folder)));
    }
    for mode in LETTERS {
        assert!(all.contains(&Step::Letter(mode)));
    }
    for tab in settings::tabs::all() {
        assert!(all.contains(&Step::Settings(tab.id)), "no picture of the settings page {}", tab.id);
    }
    assert!(all.len() >= 60, "dozens of pictures were asked for, and the tour has {}", all.len());
}

#[test]
fn settings_pages_follow_one_another() {
    // Leaving a settings page for another keeps settings open: only the last one closes them.
    let all = steps(3);
    let pages: Vec<usize> = all.iter().enumerate().filter(|(_, step)| matches!(step, Step::Settings(_))).map(|(at, _)| at).collect();
    let (Some(first), Some(last)) = (pages.first(), pages.last()) else { panic!("no settings pages") };
    assert_eq!(last - first + 1, pages.len());
}

#[test]
fn the_window_comes_back_from_full_screen_before_the_looks() {
    let all = steps(1);
    let full = all.iter().position(|step| *step == Step::FullScreen);
    let first_look = all.iter().position(|step| matches!(step, Step::Look(..)));
    assert!(full.is_some() && first_look == full.map(|at| at + 1));
    assert_eq!(all.iter().rev().find(|step| matches!(step, Step::Look(..))), Some(&Step::Look("light", 1, 0)));
    assert_eq!(all.iter().rev().find(|step| matches!(step, Step::Zoom(..))), Some(&Step::Zoom("zoom-100", 100)));
}

#[test]
fn names_are_unique_plain_and_sort_in_order() {
    let all = steps(9);
    let names: Vec<String> = all.iter().enumerate().map(|(index, step)| step_text(index, step)).collect();
    for name in &names {
        assert!(name.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'), "{name}");
    }
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(sorted, names, "the pictures sort in the tour's order");
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "two pictures under one name");
    assert_eq!(names.first().map(String::as_str), Some("01-mail-1"));
}

#[test]
fn a_one_mail_inbox_still_has_a_tour() {
    let all = steps(1);
    assert_eq!(all.iter().filter(|step| matches!(step, Step::Mail(_))).count(), 1);
    assert!(all.len() > 40);
}

#[test]
fn the_tour_zooms_only_to_what_the_setting_offers() {
    for (_, zoom) in ZOOMS {
        assert!(crate::look::ZOOMS.contains(&zoom), "the setting has no zoom of {zoom}");
    }
}
