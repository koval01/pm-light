use super::*;

const MS: Duration = Duration::from_millis(1);

#[test]
fn quick_answers_never_ask() {
    let (mut watch, now) = (Watch::default(), Instant::now());
    assert!((0..50).all(|_| !watch.heard(MS * 600, now)));
}

#[test]
fn slow_answers_ask_once_and_then_rest() {
    let (mut watch, now) = (Watch::default(), Instant::now());
    let asked: Vec<bool> = (0..WINDOW).map(|_| watch.heard(MS * 9000, now)).collect();
    assert_eq!(asked, [false, false, false, false, true]);
    assert!(!watch.heard(MS * 9000, now + Duration::from_secs(60)), "asked again within the pause");
    assert!(watch.heard(MS * 9000, now + Duration::from_secs(121)));
}

/// One slow answer among quick ones is a large answer, not a slow circuit.
#[test]
fn one_slow_answer_is_not_a_slow_circuit() {
    let (mut watch, now) = (Watch::default(), Instant::now());
    for took in [600, 700, 30_000, 650, 620, 640] {
        assert!(!watch.heard(MS * took, now), "{took}");
    }
}

/// The answers that build new circuits are slow by nature, and are not counted.
#[test]
fn answers_over_new_circuits_settle_first() {
    let (mut watch, now) = (Watch::default(), Instant::now());
    watch.renewed();
    let asked: Vec<bool> = (0..SETTLING as usize + WINDOW).map(|_| watch.heard(MS * 9000, now)).collect();
    assert_eq!(asked.iter().filter(|asked| **asked).count(), 1);
    assert!(asked.last().copied().unwrap_or(false));
}

#[test]
fn only_a_quick_quad9_means_the_network_works() {
    assert!(network_works(Some(MS * 800)));
    assert!(!network_works(Some(MS * 7000)));
    assert!(!network_works(None));
}
