use super::*;
use blitz_traits::net::Url;
use std::sync::Arc;

fn png(width: u32, height: u32) -> Vec<u8> {
    let picture = image::GrayImage::from_pixel(width, height, image::Luma([200]));
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(picture).write_to(&mut out, image::ImageFormat::Png).expect("a PNG");
    out.into_inner()
}

fn address(bytes: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// What a handler was given.
struct Caught(Arc<parking_lot::Mutex<Option<usize>>>);

impl NetHandler for Caught {
    fn bytes(self: Box<Self>, _address: String, bytes: Bytes) {
        *self.0.lock() = Some(bytes.len());
    }
}

fn ask(pictures: &Pictures, address: &str) -> Option<usize> {
    let caught = Arc::new(parking_lot::Mutex::new(None));
    let url = Url::parse(address).expect("an address");
    pictures.fetch(0, Request::get(url), Box::new(Caught(caught.clone())));
    *caught.lock()
}

#[test]
fn decodes_data_addresses_as_written() {
    assert_eq!(decode("data:image/png;base64,QUJD").as_deref(), Some(&b"ABC"[..]));
    assert_eq!(decode("DATA:image/png;BASE64,QUI").as_deref(), Some(&b"AB"[..]));
    assert_eq!(decode("data:image/png;base64,QU\n JD").as_deref(), Some(&b"ABC"[..]));
    assert_eq!(decode("data:image/png;base64,QU%4AD").as_deref(), Some(&b"ABC"[..]));
    assert_eq!(decode("data:text/plain,a%20b").as_deref(), Some(&b"a b"[..]));
}

#[test]
fn decodes_nothing_that_is_no_data_address() {
    for address in ["https://example.com/a.png", "cid:a@b", "data:image/png;base64", "data:image/png;base64,***", ""] {
        assert_eq!(decode(address), None, "{address}");
    }
}

#[test]
fn reads_a_pictures_size_without_decoding_it() {
    assert_eq!(pixels_of(&png(40, 25)), Some(1000));
    assert_eq!(pixels_of(b"not a picture"), None);
}

#[test]
fn serves_a_picture_the_page_carries() {
    let pictures = Pictures::default();
    let bytes = png(10, 10);
    assert_eq!(ask(&pictures, &address(&bytes)), Some(bytes.len()));
    assert_eq!((pictures.served(), pictures.refused()), (1, 0));
}

#[test]
fn answers_anything_else_with_nothing() {
    let pictures = Pictures::default();
    for other in ["https://example.com/a.png", "http://example.com/", "file:///etc/hosts", "data:text/plain,hello"] {
        assert_eq!(ask(&pictures, other), Some(0), "{other}");
    }
    assert_eq!((pictures.served(), pictures.refused()), (0, 4));
}

#[test]
fn refuses_one_picture_past_its_size() {
    let pictures = Pictures::default();
    assert_eq!(ask(&pictures, &address(&png(4001, 4000))), Some(0));
    assert_eq!(ask(&pictures, &address(&png(4000, 4000))).map(|length| length > 0), Some(true));
}

#[test]
fn refuses_pictures_past_the_pages_budget() {
    let pictures = Pictures::default();
    let large = address(&png(4000, 4000));
    for _ in 0..2 {
        assert!(ask(&pictures, &large).is_some_and(|length| length > 0));
    }
    // Two of sixteen million pixels leave eight: the third goes, a small one still fits.
    assert_eq!(ask(&pictures, &large), Some(0));
    assert!(ask(&pictures, &address(&png(100, 100))).is_some_and(|length| length > 0));
    assert_eq!((pictures.served(), pictures.refused()), (3, 1));
}

#[test]
fn a_header_that_claims_too_much_is_refused_before_decoding() {
    let mut lying = png(1, 1);
    // The IHDR width and height, at bytes 16 to 24 of every PNG.
    if let Some(size) = lying.get_mut(16..24) {
        size.copy_from_slice(&[0, 1, 0x86, 0xa0, 0, 1, 0x86, 0xa0]);
    }
    let pictures = Pictures::default();
    assert_eq!(ask(&pictures, &address(&lying)), Some(0));
}
