use super::*;

fn given(words: &[&str]) -> Vec<String> {
    after_own_name(words.iter().map(|word| (*word).to_string())).collect()
}

#[test]
fn the_programs_own_name_is_never_a_switch() {
    assert_eq!(given(&["--hidden", "mailto:someone@example.com"]), ["mailto:someone@example.com"]);
    assert_eq!(given(&["--relaunched"]), [] as [String; 0]);
    assert_eq!(given(&["--restarted=1:1000", "--hidden"]), ["--hidden"]);
}

#[test]
fn a_start_with_nothing_given_has_no_arguments() {
    assert_eq!(given(&[]), [] as [String; 0]);
    assert_eq!(given(&["/Applications/Proton Mail Light.app/Contents/MacOS/proton-mail-light"]), [] as [String; 0]);
}

#[test]
fn every_word_after_the_name_comes_through_in_order() {
    assert_eq!(
        given(&["program", "--soak-minutes=60", "--soak-through-tor", ""]),
        ["--soak-minutes=60", "--soak-through-tor", ""]
    );
}

#[test]
fn the_running_program_has_a_file() {
    assert!(file().is_some_and(|path| path.is_absolute()));
}
