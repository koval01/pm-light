//! Crafted mails through the cleaner and then a page, as the reader takes them. Each found a fault in Blitz or a cost
//! past bearing, run in Docker (docs/BLITZ.md); now each must be refused, cut, or drawn in bounded time.

use super::super::*;
use super::{PANE, fonts};
use crate::html::protect;
use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

/// Far longer than an ordinary mail takes: past it a mail is an attack on the CPU.
const PATIENCE: Duration = Duration::from_secs(20);

/// A mail cleaned, opened and painted on a thread of `STACK`, as the reader does it. Whether it was refused.
fn read(name: &str, mail: &str) -> Option<Refused> {
    let cleaned = protect(mail).html;
    let html = format!("<!doctype html><html><body>{cleaned}</body></html>");
    let start = Instant::now();
    let worker = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || match Page::open(&html, &fonts(), PANE) {
            Ok(mut page) => {
                let _ = page.paint(0, PANE.height);
                None
            }
            Err(refused) => Some(refused),
        })
        .expect("a thread");
    let outcome = worker.join().expect("no panic in Blitz");
    let took = start.elapsed();
    println!("page: {name} {:?} in {} ms", outcome, took.as_millis());
    assert!(took < PATIENCE, "{name} took {took:?}");
    outcome
}

fn white_png(side: u32) -> String {
    use base64::Engine as _;
    let picture = image::GrayImage::from_pixel(side, side, image::Luma([255]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(picture).write_to(&mut out, image::ImageFormat::Png).expect("a PNG");
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(out.into_inner()))
}

#[test]
#[ignore = "hostile input: run by scripts/attacks.sh in a container"]
fn nesting_that_passes_the_cleaner_is_refused_before_layout() {
    // A table counts once in the cleaner's depth and is four elements deep: 2,051 levels overflowed 2 MB.
    assert_eq!(read("512 tables", &format!("{}x", "<table><tr><td>".repeat(512))), Some(Refused::TooDeep));
    assert_eq!(read("512 lists", &format!("{}x", "<ul><li>".repeat(512))), Some(Refused::TooDeep));
    assert_eq!(read("500 dl", &format!("{}x", "<dl><dd>".repeat(500))), Some(Refused::TooDeep));
    // Within the limit, laid out and painted.
    assert_eq!(read("120 tables", &format!("{}x", "<table><tr><td>".repeat(120))), None);
    assert_eq!(read("500 divs", &format!("{}x{}", "<div>".repeat(500), "</div>".repeat(500))), None);
    assert_eq!(read("500 opacity", &"<div style=\"opacity:.99;z-index:1;position:relative\">".repeat(500)), None);
}

#[test]
#[ignore = "hostile input: run by scripts/attacks.sh in a container"]
fn spans_and_colours_that_made_blitz_panic_are_drawn() {
    assert_eq!(
        read("colspan", "<table><tr><td colspan=\"40000\">a</td><td colspan=\"65535\">b</td></tr></table>"),
        None
    );
    assert_eq!(read("rowspan", &"<tr><td rowspan=\"65534\">a</td><td colspan=\"1000\">b</td></tr>".repeat(50)), None);
    assert_eq!(
        read("colour", "<table bgcolor=\"#a\u{e9}\u{e9}1\"><tr><td bgcolor=\"#\u{e9}1\">x</td></tr></table>"),
        None
    );
}

#[test]
#[ignore = "hostile input: run by scripts/attacks.sh in a container"]
fn pictures_stay_within_the_budget() {
    let large = white_png(4000);
    let mail = (0..16).fold(String::new(), |mut mail, i| {
        let _ = write!(mail, "<img src=\"{large}#{i}\" width=\"10\" height=\"10\">");
        mail
    });
    assert_eq!(read("sixteen large pictures", &mail), None);
    assert_eq!(read("one huge picture", &format!("<img src=\"{}\">", white_png(11_000))), None);
}

#[test]
#[ignore = "hostile input: run by scripts/attacks.sh in a container"]
fn shadows_and_size_are_bounded() {
    let layers: Vec<String> = (0..1000).map(|i| format!("{i}px {i}px {}px #000", i * 10)).collect();
    assert_eq!(
        read("shadows", &format!("<div style=\"width:200px;height:200px;box-shadow:{}\">x</div>", layers.join(","))),
        None
    );
    assert_eq!(
        read("cells", &format!("<table><tr>{}</table>", "<td>x".repeat(59_000))),
        Some(Refused::TooManyElements)
    );
    assert_eq!(read("text", &format!("<p>{}</p>", "lorem ipsum ".repeat(300_000))), Some(Refused::TooMuchText));
    assert_eq!(
        read(
            "huge sizes",
            "<p style=\"font-size:1000000px;letter-spacing:-1000000000px;border:1000000px solid\">x</p>"
        ),
        None
    );
    assert_eq!(read("floats", &"<div style=\"float:left;width:3px;height:3px\"></div>".repeat(10_000)), None);
}

#[test]
#[ignore = "hostile input: run by scripts/attacks.sh in a container"]
fn a_page_reaches_for_nothing() {
    let mail = r#"<style>@import url("https://example.com/a.css"); p { background: url(https://example.com/b.png) } @font-face { font-family: X; src: url(https://example.com/f.woff) }</style><link rel="stylesheet" href="https://example.com/c.css"><img src="https://example.com/d.png"><img src="cid:e@example"><img src="file:///etc/hosts"><iframe src="https://example.com/f.html"></iframe><p style="background-image:url('https://example.com/g.png')">x</p><img src="//example.com/i.png"><img src="/j.png"><div style="background:url('')">y</div><div style="background:url('#x')">z</div>"#;
    let cleaned = protect(mail).html;
    let html = format!("<!doctype html><html><body>{cleaned}</body></html>");
    let worker = std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(move || Page::open(&html, &fonts(), PANE).map(|page| page.pictures()))
        .expect("a thread");
    let (served, refused) = worker.join().expect("no panic").expect("a page");
    println!("page: requests served {served}, refused {refused}");
    assert_eq!(served, 0);
}
