use super::*;

#[test]
fn only_its_own_argument_asks_for_it() {
    let arguments = |all: &[&str]| all.iter().map(ToString::to_string).collect::<Vec<_>>().into_iter();
    assert!(asked_in(arguments(&["--check-picture=p.png", "--check-window"])));
    assert!(!asked_in(arguments(&["--check-window=x"])));
    assert!(!asked_in(arguments(&[])));
}

#[test]
fn the_steps_come_in_order_five_seconds_apart() {
    assert!(STEPS.windows(2).all(|pair| pair[1].0.checked_sub(pair[0].0) == Some(Duration::from_secs(5))));
    assert_eq!(STEPS.last().map(|step| step.1), Some("as it opened"));
}
