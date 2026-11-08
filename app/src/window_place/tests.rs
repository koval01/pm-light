use super::*;

fn at(x: i32, y: i32) -> WindowPlace {
    WindowPlace { x, y, width: 800, height: 600 }
}

#[test]
fn a_place_on_a_display_is_kept() {
    assert!(is_on(&at(100, 100), (0, 0, 1920, 1080)));
    assert!(is_on(&at(2000, 100), (1920, 0, 1920, 1080)));
    assert!(is_on(&at(-500, 100), (-1920, 0, 1920, 1080)));
}

#[test]
fn a_place_on_a_display_that_is_gone_is_not() {
    assert!(!is_on(&at(4000, 100), (0, 0, 1920, 1080)));
    assert!(!is_on(&at(100, 3000), (0, 0, 1920, 1080)));
}

#[test]
fn a_window_needs_some_of_itself_on_the_display() {
    assert!(!is_on(&at(1900, 100), (0, 0, 1920, 1080)));
    assert!(!is_on(&at(100, -10), (0, 0, 1920, 1080)));
}

const SCREEN: Frame = Frame { x: 0, y: 0, width: 1024, height: 768 };

fn window(x: i32, y: i32, width: u32, height: u32) -> Frame {
    // A frame 16 wider and 39 taller than its content: borders and a title bar.
    Frame { x, y, width: width + 16, height: height + 39 }
}

#[test]
fn a_window_wholly_on_its_display_stays() {
    let outer = window(50, 50, 900, 580);
    assert_eq!(fitted(Frame { width: 1920, height: 1080, ..SCREEN }, outer, (900, 580), (900, 580), 80), None);
}

#[test]
fn a_window_larger_than_a_small_display_shrinks_and_is_centred() {
    let outer = window(112, 100, 1280, 820);
    let frame = fitted(SCREEN, outer, (1280, 820), (900, 580), 80).unwrap();
    assert_eq!((frame.width, frame.height), (1008, 649), "the room left, less the frame");
    assert_eq!(frame.x, 0);
    assert_eq!(frame.y, 0);
    assert!(frame.x + i32::try_from(frame.width + 16).unwrap() <= 1024);
    assert!(frame.y + i32::try_from(frame.height + 39).unwrap() <= 768 - 80);
}

#[test]
fn a_window_partly_off_its_display_comes_back_whole() {
    let outer = window(1500, 300, 900, 580);
    let frame = fitted(Frame { width: 1920, height: 1080, ..SCREEN }, outer, (900, 580), (900, 580), 80).unwrap();
    assert_eq!((frame.width, frame.height), (900, 580), "it fits, so it keeps its size");
    assert_eq!((frame.x, frame.y), ((1920 - 916) / 2, (1000 - 619) / 2));
}

#[test]
fn a_display_smaller_than_the_least_size_keeps_the_least_size() {
    let outer = window(0, 0, 1280, 820);
    let frame = fitted(Frame { width: 800, height: 600, ..SCREEN }, outer, (1280, 820), (900, 580), 80).unwrap();
    assert_eq!((frame.width, frame.height), (900, 580));
    assert_eq!((frame.x, frame.y), (0, 0));
}

#[test]
fn a_second_display_is_measured_from_its_own_corner() {
    let display = Frame { x: 1920, y: 0, width: 1024, height: 768 };
    let frame = fitted(display, window(2000, 100, 1280, 820), (1280, 820), (900, 580), 80).unwrap();
    assert_eq!(frame.x, 1920);
}
