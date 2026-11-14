use super::*;

#[test]
fn a_program_in_a_bundle_starts_as_the_bundle() {
    let program = Path::new("/Applications/Proton Mail Light.app/Contents/MacOS/proton-mail-light");
    assert_eq!(bundle_of(program), Path::new("/Applications/Proton Mail Light.app"));
}

#[test]
fn a_lone_program_starts_as_itself() {
    assert_eq!(bundle_of(Path::new("/opt/proton-mail-light")), Path::new("/opt/proton-mail-light"));
}
