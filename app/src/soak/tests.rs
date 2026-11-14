use super::*;

#[test]
fn every_action_can_be_chosen() {
    let all = [
        Action::OpenMail,
        Action::Step,
        Action::Scroll,
        Action::Folder,
        Action::Search,
        Action::Settings,
        Action::Letter,
        Action::Preview,
        Action::Palette,
        Action::Contacts,
        Action::Look,
        Action::Zoom,
        Action::Resize,
        Action::Maximise,
        Action::Reload,
        Action::ShowImages,
        Action::MailAct,
    ];
    for action in all {
        assert!(
            WEIGHTS.iter().any(|(weighed, weight)| *weighed == action && *weight > 0),
            "{action:?} is never chosen"
        );
    }
    assert_eq!(WEIGHTS.len(), all.len());
}

#[test]
fn a_pick_always_lands_in_its_list() {
    for pick in [0, 1, 9, 10, 11, u64::MAX] {
        assert!(FOLDERS.contains(&one_of(&FOLDERS, pick)));
    }
    assert_eq!(one_of(&[], 7), "");
}

#[test]
fn one_seed_gives_one_run() {
    let run = |seed: u64| {
        let mut random = StdRng::seed_from_u64(seed);
        (0..50)
            .filter_map(|_| WEIGHTS.choose_weighted(&mut random, |(_, weight)| *weight).ok().map(|(action, _)| *action))
            .collect::<Vec<_>>()
    };
    assert_eq!(run(7), run(7), "a seed must repeat its run");
    assert_ne!(run(7), run(8));
}

#[test]
fn nothing_the_run_does_waits_on_a_person_or_writes_to_the_account() {
    // What the server would change is asked for on purpose, and refused in a check.
    assert!(MAIL_ACTS.iter().all(|act| ["star", "unread", "archive"].contains(act)));
    assert!(!LETTERS.contains(&"send"));
}
