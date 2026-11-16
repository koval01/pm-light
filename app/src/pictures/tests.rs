use super::*;
use std::io::Cursor;

/// A picture of one colour as Proton sends one: a PNG in a data URL.
fn png_url(width: u32, height: u32, colour: [u8; 4]) -> String {
    let picture = RgbaImage::from_pixel(width, height, image::Rgba(colour));
    let mut bytes = Vec::new();
    picture.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).expect("a PNG");
    format!("data:image/png;base64,{}", STANDARD.encode(bytes))
}

const SVG: &str =
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="red"/></svg>"#;

fn alpha(pixels: &SharedPixelBuffer<Rgba8Pixel>, x: u32, y: u32) -> u8 {
    let at = usize::try_from(y * pixels.width() + x).expect("an index");
    pixels.as_slice().get(at).expect("a pixel inside").a
}

#[test]
fn a_picture_in_a_data_url_is_read_with_its_size() {
    let Some(Logo::Pixels(pixels, width, height)) = logo_of(&png_url(3, 2, [10, 20, 30, 255])) else {
        panic!("no pixels");
    };
    assert_eq!((width, height, pixels.len()), (3, 2, 3 * 2 * 4));
    assert_eq!(pixels.get(..4), Some(&[10, 20, 30, 255][..]));
}

#[test]
fn a_vector_logo_is_kept_as_it_came() {
    let url = format!("data:image/svg+xml;base64,{}", STANDARD.encode(SVG));
    assert!(matches!(logo_of(&url), Some(Logo::Vector(bytes)) if bytes == SVG.as_bytes()));
}

#[test]
fn what_is_not_a_picture_in_a_data_url_is_nothing() {
    for hostile in [
        "",
        "https://tracker.example/logo.png",
        "data:image/png,not-base64-at-all",
        "data:image/png;base64,***",
        "data:image/png;base64,",
        &format!("data:image/png;base64,{}", STANDARD.encode("not a picture")),
        // A PNG's signature with nothing after it.
        &format!("data:image/png;base64,{}", STANDARD.encode([0x89, b'P', b'N', b'G', 13, 10, 26, 10])),
    ] {
        assert!(logo_of(hostile).is_none(), "{hostile:?}");
    }
}

#[test]
fn a_logo_becomes_a_disc_of_the_side_asked_for_with_its_corners_clear() {
    for (width, height) in [(1, 1), (40, 40), (300, 20), (20, 300)] {
        let logo = logo_of(&png_url(width, height, [255, 255, 255, 255])).expect("a logo");
        for side in [1, 34, 76] {
            let pixels = picture_pixels(&logo, side).expect("pixels");
            assert_eq!((pixels.width(), pixels.height()), (side, side));
            if side > 2 {
                assert_eq!(alpha(&pixels, 0, 0), 0, "a corner is drawn, {width}x{height} at {side}");
                assert_eq!(alpha(&pixels, side - 1, side - 1), 0);
                assert_eq!(alpha(&pixels, side / 2, side / 2), 255, "the middle is not");
            }
        }
    }
}

/// White on white would vanish: the ring keeps the disc's edge darker than its middle.
#[test]
fn a_white_logo_has_a_ring_around_it() {
    let logo = logo_of(&png_url(40, 40, [255, 255, 255, 255])).expect("a logo");
    let pixels = picture_pixels(&logo, 76).expect("pixels");
    let at = |x: u32, y: u32| pixels.as_slice().get(usize::try_from(y * 76 + x).expect("an index")).copied();
    let middle = at(38, 38).expect("the middle");
    let edge = at(38, 1).expect("the top edge");
    assert!(edge.r < middle.r && edge.a > 0, "middle {middle:?}, edge {edge:?}");
}

#[test]
fn a_vector_logo_is_drawn_into_a_disc_too() {
    let url = format!("data:image/svg+xml;base64,{}", STANDARD.encode(SVG));
    let pixels = picture_pixels(&logo_of(&url).expect("a logo"), 34).expect("pixels");
    let middle = pixels.as_slice().get(17 * 34 + 17).copied().expect("the middle");
    assert!(middle.r > 200 && middle.g < 60, "{middle:?}");
    assert!(picture_pixels(&Logo::Vector(b"<svg".to_vec()), 34).is_none(), "a broken vector was drawn");
}

#[test]
fn a_thumbnail_keeps_its_colour_and_loses_only_its_corners() {
    let logo = logo_of(&png_url(64, 48, [0, 128, 0, 255])).expect("a picture");
    let pixels = thumbnail_pixels(&logo, 34).expect("pixels");
    assert_eq!((pixels.width(), pixels.height()), (34, 34));
    assert_eq!(alpha(&pixels, 0, 0), 0, "a corner is drawn");
    assert_eq!(alpha(&pixels, 33, 0), 0);
    assert_eq!(alpha(&pixels, 17, 0), 255, "the middle of an edge is cut");
    let middle = pixels.as_slice().get(17 * 34 + 17).copied().expect("the middle");
    assert_eq!((middle.r, middle.g, middle.b, middle.a), (0, 128, 0, 255));
    assert!(thumbnail_pixels(&Logo::Vector(SVG.as_bytes().to_vec()), 34).is_none(), "a vector has no thumbnail");
}

#[test]
fn pixels_that_do_not_fill_their_size_are_nothing() {
    assert!(picture_pixels(&Logo::Pixels(vec![0; 3], 2, 2), 34).is_none());
    assert!(thumbnail_pixels(&Logo::Pixels(vec![0; 3], 2, 2), 34).is_none());
}
