use super::*;

fn arguments(all: &[&str]) -> impl Iterator<Item = String> {
    all.iter().map(ToString::to_string).collect::<Vec<_>>().into_iter()
}

#[test]
fn the_file_comes_from_its_argument() {
    assert_eq!(asked_in(arguments(&["--check-account=out/a"])), Some(PathBuf::from("out/a")));
}

#[test]
fn no_argument_or_an_empty_one_asks_for_nothing() {
    assert_eq!(asked_in(arguments(&[])), None);
    assert_eq!(asked_in(arguments(&["--check-account="])), None);
    assert_eq!(asked_in(arguments(&["--check-picture=p.png"])), None, "the picture check is another one");
}

#[test]
fn the_report_holds_numbers_only() {
    let report = report(14, 9, 1440, 2210);
    assert_eq!(report, "folders=14\ninbox rows=9\nwidth=1440\nheight=2210\n");
    assert!(report.chars().all(|c| c.is_ascii_alphanumeric() || c == '=' || c == '\n' || c == ' '));
}
