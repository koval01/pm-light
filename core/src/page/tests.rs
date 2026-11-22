use super::*;

/// The app's own font, so that a page lays out alike on a system with no fonts.
const INTER: &[u8] = include_bytes!("../../../app/ui/fonts/InterVariable.ttf");

pub fn fonts() -> Fonts {
    Fonts::only(INTER)
}

pub const PANE: Sight = Sight { width: 1200, height: 800, scale: 2.0 };

fn page(body: &str) -> Page {
    let html = format!(
        "<!doctype html><html><head><style>body{{margin:0;font:16px sans-serif}}</style></head><body>{body}</body></html>"
    );
    Page::open(&html, &fonts(), PANE).expect("a page within the limits")
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = usize::try_from((y * width + x) * 4).expect("an index");
    rgba[at..at + 4].try_into().expect("four bytes")
}

#[test]
fn paints_the_pane_on_white() {
    let mut page = page(r#"<div style="width:100px;height:100px;background:#0000ff"></div><p>words</p>"#);
    let rgba = page.paint(0, 400);
    assert_eq!(rgba.len(), 1200 * 400 * 4);
    assert_eq!(pixel(&rgba, 1200, 50, 50), [0, 0, 255, 255]);
    assert_eq!(pixel(&rgba, 1200, 1100, 390), [255, 255, 255, 255]);
}

#[test]
fn paints_a_band_further_down_the_page() {
    let mut page = page(r#"<div style="height:1000px"></div><div style="height:100px;background:#ff0000"></div>"#);
    assert!(page.height() >= 2200, "{}", page.height());
    let rgba = page.paint(2000, 100);
    assert_eq!(pixel(&rgba, 1200, 10, 50), [255, 0, 0, 255]);
}

#[test]
fn never_paints_more_rows_than_the_renderer_counts() {
    let mut page = page(r#"<div style="height:20000px"></div>"#);
    let rgba = page.paint(0, 100_000);
    assert_eq!(rgba.len(), usize::try_from(1200 * TALLEST_BAND * 4).expect("a length"));
}

#[test]
fn a_page_wider_than_its_pane_is_drawn_smaller() {
    let narrow = page("<p>short</p>");
    assert!((narrow.scale - 2.0).abs() < f64::EPSILON);
    let wide = page(r#"<table width="1200"><tr><td>wide</td></tr></table>"#);
    assert!((wide.scale - 1.0).abs() < 0.01, "{}", wide.scale);
    let huge = page(r#"<table width="6000"><tr><td>huge</td></tr></table>"#);
    // Never below half: 2.0 screen pixels to a CSS pixel become 1.0.
    assert!((huge.scale - 1.0).abs() < f64::EPSILON, "{}", huge.scale);
}

#[test]
fn a_new_width_lays_out_again() {
    let mut wide = page(r#"<table width="1200"><tr><td>wide</td></tr></table>"#);
    wide.resize(Sight { width: 2400, ..PANE });
    assert!((wide.scale - 2.0).abs() < f64::EPSILON, "{}", wide.scale);
    wide.resize(Sight { height: 300, width: 2400, ..PANE });
    assert_eq!(wide.sight().height, 300);
}

#[test]
fn finds_the_link_under_a_point() {
    let page = page(
        r#"<a href="https://example.com/path?q=1" style="display:block;height:50px">one</a><a href="mailto:a@example.com" style="display:block;height:50px">two</a><a href="/relative" style="display:block;height:50px">three</a><p>no link</p>"#,
    );
    assert_eq!(page.link_at(20.0, 20.0).as_deref(), Some("https://example.com/path?q=1"));
    assert_eq!(page.link_at(20.0, 120.0).as_deref(), Some("mailto:a@example.com"));
    assert_eq!(page.link_at(20.0, 220.0), None);
    assert_eq!(page.link_at(20.0, 320.0), None);
}

#[test]
fn the_pointer_shows_a_hand_over_a_link_and_a_caret_over_text() {
    let mut page = page(
        r#"<a href="https://example.com/" style="display:block;height:50px">link</a><p>Some text here</p><div style="height:400px"></div>"#,
    );
    assert_eq!(page.cursor_at(20.0, 20.0), Cursor::Pointer);
    assert_eq!(page.cursor_at(1100.0, 700.0), Cursor::Default);
}

#[test]
fn selects_the_text_dragged_over() {
    let mut page = page("<p>Hello selected world</p>");
    let (top, line) = (32.0, 16.0 * 2.0);
    page.press(1.0, top + line / 2.0);
    assert!(page.drag(1150.0, top + line / 2.0));
    assert_eq!(page.selected_text().as_deref(), Some("Hello selected world"));
    page.clear_selection();
    assert_eq!(page.selected_text(), None);
}

#[test]
fn carries_its_own_pictures_and_nothing_else() {
    let picture = "data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACwAAAAAAQABAAACAkQBADs=";
    let page = page(&format!(
        r#"<img src="{picture}" width="10" height="10"><img src="https://example.com/a.png"><div style="background:url('https://example.com/b.png');height:5px"></div><div style="background:url('#x');height:5px"></div>"#
    ));
    let (served, refused) = page.pictures();
    assert_eq!(served, 1);
    assert!(refused >= 2, "{refused}");
}

#[test]
fn refuses_a_page_nested_too_deep() {
    let html = format!("{}x", "<div>".repeat(DEEPEST + 1));
    assert_eq!(Page::open(&html, &fonts(), PANE).err(), Some(Refused::TooDeep));
    let html = format!("{}x", "<div>".repeat(DEEPEST - 2));
    assert!(Page::open(&html, &fonts(), PANE).is_ok());
}

#[test]
fn refuses_a_page_of_too_many_elements() {
    let html = "<b>x</b>".repeat(MOST_ELEMENTS);
    assert_eq!(Page::open(&html, &fonts(), PANE).err(), Some(Refused::TooManyElements));
}

#[test]
fn refuses_a_page_of_too_much_text() {
    let html = format!("<p>{}</p>", "a".repeat(MOST_TEXT + 1));
    assert_eq!(Page::open(&html, &fonts(), PANE).err(), Some(Refused::TooMuchText));
}

#[test]
fn whole_counts_round_up_within_bounds() {
    assert_eq!(whole(10.2, 100), 11);
    assert_eq!(whole(1e12, 100), 100);
    assert_eq!(whole(-5.0, 100), 0);
    assert_eq!(whole(f64::NAN, 100), 0);
    assert_eq!(whole(f64::INFINITY, 100), 0);
}

/// What scrolling costs: a screen of a long letter painted at one place after another down the page.
#[test]
#[ignore = "a measurement: cargo test --release -p proton-core --lib page::tests::scrolling_cost -- --ignored --nocapture"]
fn scrolling_cost() {
    let paragraph = "<p>The report shows steady growth across every region while costs stayed flat, and the team shipped four releases on time despite the move to a new office in spring. <a href=\"https://example.com/\">A link</a> and <b>bold words</b> follow.</p>";
    let mut page = Page::open(
        &format!("<div style=\"font:15px serif\">{}</div>", paragraph.repeat(400)),
        &fonts(),
        Sight { width: 1360, height: 1200, scale: 2.0 },
    )
    .expect("a page");
    let height = page.height();
    let mut times: Vec<u128> = (0..20)
        .map(|step| {
            let top = height.saturating_sub(1200) / 20 * step;
            let start = std::time::Instant::now();
            let rgba = page.paint(top, 1200);
            assert!(
                rgba.as_chunks::<4>().0.iter().any(|pixel| *pixel != [255, 255, 255, 255]),
                "screen {step} is blank"
            );
            start.elapsed().as_micros()
        })
        .collect();
    times.sort_unstable();
    println!(
        "page: {height} px; a screen of 1360x1200 painted in {} us at the median, {} us at worst",
        times[10], times[19]
    );
}

#[cfg(test)]
mod hostile;

/// Newsletters stack their columns on a narrow screen by making each cell a block, under a media query.
#[test]
fn cells_made_blocks_are_still_drawn() {
    let mut page = page(
        r#"<table width="100%"><tr><td style="display:block;background:#ff0000;height:40px">a</td><td style="display:block;background:#0000ff;height:40px">b</td></tr></table>"#,
    );
    let rgba = page.paint(0, 400);
    let red = rgba.as_chunks::<4>().0.iter().filter(|pixel| **pixel == [255, 0, 0, 255]).count();
    let blue = rgba.as_chunks::<4>().0.iter().filter(|pixel| **pixel == [0, 0, 255, 255]).count();
    assert!(red > 1000 && blue > 1000, "red {red}, blue {blue}");
    // Stacked, each across the table, as in a browser: red above blue at the middle of the pane.
    assert_eq!(pixel(&rgba, 1200, 600, 40), [255, 0, 0, 255]);
    assert_eq!(pixel(&rgba, 1200, 600, 130), [0, 0, 255, 255]);
}
