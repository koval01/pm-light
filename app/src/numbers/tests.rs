use super::*;

#[test]
fn a_share_stays_between_nothing_and_all() {
    assert!(share(0, 0).abs() < f32::EPSILON);
    assert!((share(1, 4) - 0.25).abs() < 1e-6);
    assert!((share(9, 4) - 1.0).abs() < f32::EPSILON);
    assert!((share(u64::MAX, u64::MAX) - 1.0).abs() < f32::EPSILON);
}

#[test]
fn out_of_range_values_are_held_at_the_edges() {
    assert_eq!(channel(-3.0), 0);
    assert_eq!(channel(300.0), 255);
    assert_eq!(channel(f32::NAN), 0);
    assert_eq!(pixels(-1.0), 0);
    assert_eq!(pixels(1e9), 65_535);
    assert_eq!(float(70_000), 65_535.0);
}
