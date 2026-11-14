use super::*;

#[test]
fn only_a_beat_late_by_more_than_three_frames_counts() {
    assert_eq!(held(Duration::from_millis(16)), None);
    assert_eq!(held(Duration::from_millis(60)), None);
    assert_eq!(held(Duration::from_millis(100)), Some(Duration::from_millis(84)));
    assert_eq!(held(Duration::ZERO), None);
}
