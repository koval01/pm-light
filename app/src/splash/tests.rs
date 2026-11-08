use super::{FLIGHT, FRAMES, LARGE, SMALL, drawn, flight_size, frame_svg, logo_svg, pose};

/// A turn ends where the next begins: shut, at rest, the letter out of sight. Otherwise every loop would jump.
#[test]
fn a_turn_ends_where_it_began() {
    let (first, last) = (pose(0.0), pose(1.0));
    assert_eq!(first, last);
    assert!((first.fold - 1.0).abs() < 1e-6 && first.lift.abs() < 1e-6 && first.squash.abs() < 1e-6);
    assert!(first.letter_shown.abs() < 1e-6, "the letter shows before the flap opens");
    let almost = pose(1.0 - 1.0 / crate::numbers::float(FRAMES));
    assert!((almost.fold - 1.0).abs() < 1e-3 && almost.lift < 0.02, "{almost:?}");
}

/// The flap opens before the letter goes in, and shuts after it is in.
#[test]
fn the_letter_goes_in_while_the_flap_is_open() {
    assert!(pose(0.12).fold < -0.99, "{:?}", pose(0.12));
    assert!(pose(0.34).letter_y > 7.9);
    assert!(pose(0.46).fold > 0.99);
    assert!(pose(0.78).lift > 0.99, "{:?}", pose(0.78));
    for step in 0..100 {
        let moment = pose(crate::numbers::float(step) / 100.0);
        assert!((-1.0..=1.0).contains(&moment.fold) && (0.0..=1.0).contains(&moment.lift), "{moment:?}");
    }
}

#[test]
fn the_flight_goes_from_the_splash_to_the_logo_in_even_steps() {
    assert!((flight_size(0) - LARGE).abs() < 1e-4);
    assert!((flight_size(FLIGHT - 1) - SMALL).abs() < 1e-4);
    let sizes: Vec<f32> = (0..FLIGHT).map(flight_size).collect();
    assert!(sizes.windows(2).all(|pair| pair[1] < pair[0]));
}

/// Every frame is a drawing the SVG reader takes, at the pixels asked for.
#[test]
fn every_frame_draws_at_its_pixels() {
    for frame in [0, 5, 20, 40, 60, 77] {
        let moment = pose(crate::numbers::float(frame) / crate::numbers::float(FRAMES));
        let picture = drawn(&frame_svg(&moment, 176, 242)).expect("a frame");
        assert_eq!((picture.width(), picture.height()), (176, 242), "frame {frame}");
    }
    let logo = drawn(&logo_svg(52)).expect("the logo");
    assert_eq!((logo.width(), logo.height()), (52, 52));
}
