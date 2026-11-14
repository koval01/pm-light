use super::is_bundled;

#[test]
fn only_a_program_inside_an_app_bundle_asks_the_notification_centre() {
    assert!(is_bundled("/Applications/Proton Mail Light.app"));
    assert!(is_bundled("/Users/a/target/native/Proton Mail Light.APP"));
    assert!(!is_bundled("/Users/a/proton-mail/target/native"));
    assert!(!is_bundled("/Users/a/proton-mail/target/debug/"));
    assert!(!is_bundled(""));
}
