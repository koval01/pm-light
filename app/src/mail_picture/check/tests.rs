use super::*;

fn arguments(all: &[&str]) -> impl Iterator<Item = String> {
    all.iter().map(ToString::to_string).collect::<Vec<_>>().into_iter()
}

#[test]
fn the_file_comes_from_its_argument() {
    assert_eq!(asked_in(arguments(&["--hidden", "--check-picture=out/p.png"])), Some(PathBuf::from("out/p.png")));
}

#[test]
fn no_argument_or_an_empty_one_asks_for_nothing() {
    assert_eq!(asked_in(arguments(&[])), None);
    assert_eq!(asked_in(arguments(&["--check-picture="])), None);
    assert_eq!(asked_in(arguments(&["--check-picture", "p.png"])), None, "only with its equals sign");
    assert_eq!(asked_in(arguments(&["mailto:a@b.c"])), None);
}

#[test]
fn the_report_holds_the_pictures_size_and_what_came_of_it() {
    let mut png = Vec::new();
    image::RgbaImage::new(5, 3).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap_or_default();
    let note = report(&png, true, false);
    assert!(note.contains("width=5\n") && note.contains("height=3\n"), "{note}");
    assert!(note.contains("cut=true\n") && note.contains("copied=false\n"));
    assert!(note.contains(&format!("bytes={}\n", png.len())));
}

#[test]
fn a_report_of_what_is_no_picture_says_nothing_was_drawn() {
    assert!(report(b"not a picture", false, true).starts_with("width=0\nheight=0\n"));
}
