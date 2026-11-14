use super::*;

#[test]
fn only_the_page_served_now_is_own() {
    let served = OnePage::new("testpage");
    let first = served.serve(1, "<p>one</p>".into());
    assert!(served.is_own(&first));
    let second = served.serve(2, "<p>two</p>".into());
    assert!(served.is_own(&second));
    assert!(!served.is_own(&first), "an earlier page must not load again");
}

#[test]
fn an_earlier_page_is_still_ours_and_never_a_link() {
    let served = OnePage::new("testpage");
    let first = served.serve(1, String::new());
    let _second = served.serve(2, String::new());
    assert!(served.is_ours(&first), "an earlier page reported late went to the browser");
    let origin = served.origin();
    assert!(served.is_ours(&format!("{origin}/7")));
    // Only this protocol's own origin: anything that merely starts the same way is a link like any other.
    for other in [
        format!("{origin}.example.com/1"),
        format!("{origin}evil/1"),
        "https://example.com/testpage".to_string(),
        "http://testpage.page.example/1".to_string(),
        "testpages://page/1".to_string(),
        String::new(),
    ] {
        assert!(!served.is_ours(&other), "{other} was taken for the app's own page");
    }
}

/// What wry hands X11 for a size: physical pixels, rounded, as `XCreateSimpleWindow` takes them.
fn physical(rect: Rect, scale: f64) -> (u32, u32) {
    rect.size.to_physical::<u32>(scale).into()
}

#[test]
fn a_webview_never_gets_a_window_with_no_size() {
    let hostile = [0.0, -0.0, 0.2, 0.49, -1.0, -50_000.0, f64::NAN, f64::NEG_INFINITY, f64::MIN];
    for width in hostile {
        for height in [0.0, 300.0, f64::NAN] {
            let rect = area("a size test", 10.0, 20.0, width, height);
            for scale in [1.0, 1.25, 2.0, 3.0] {
                let (made_width, made_height) = physical(rect, scale);
                assert!(
                    made_width >= 1 && made_height >= 1,
                    "{width} by {height} at {scale} gave {made_width} by {made_height}"
                );
            }
        }
    }
}

#[test]
fn a_webview_never_grows_past_what_x11_can_hold() {
    for length in [16_385.0, 70_000.0, f64::MAX, f64::INFINITY] {
        let (width, height) = physical(area("a size test", 0.0, 0.0, length, length), 2.0);
        // X11 keeps a size in 16 bits: anything past that wraps, and 65536 wraps to nothing.
        assert!(u16::try_from(width).is_ok() && u16::try_from(height).is_ok(), "{length} gave {width} by {height}");
        assert!(width >= 1 && height >= 1);
    }
}

#[test]
fn a_webview_with_room_keeps_its_place_and_size() {
    let rect = area("a size test", 12.5, -40.0, 640.0, 480.0);
    let (x, y): (f64, f64) = rect.position.to_logical::<f64>(1.0).into();
    assert!((x - 12.5).abs() < f64::EPSILON && (y + 40.0).abs() < f64::EPSILON, "moved to {x}, {y}");
    assert_eq!(physical(rect, 1.0), (640, 480));
    assert_eq!(physical(rect, 2.0), (1280, 960));
    assert_eq!(physical(area("a size test", 0.0, 0.0, 1.0, 1.0), 1.0), (1, 1));
}

#[test]
fn a_squeezed_webview_is_reported_once_until_it_has_room_again() {
    let what = "the squeeze test";
    assert!(newly_squeezed(what, true), "the first time it has no room is news");
    assert!(!newly_squeezed(what, true), "the same squeeze again is not, or a move floods the journal");
    assert!(!newly_squeezed(what, false));
    assert!(!newly_squeezed(what, false));
    assert!(newly_squeezed(what, true), "a new squeeze after room is news again");
    assert!(!newly_squeezed("another squeeze test", false), "room is never news");
}
