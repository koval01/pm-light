use super::*;
use image::{DynamicImage, ImageFormat, RgbImage};
use std::io::Cursor;

const SECRET: &[u8] = b"GPS 50.4501 30.5234 iPhone Secret";

fn picture() -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(32, 24, |x, y| {
        image::Rgb([u8::try_from(x * 7).unwrap(), u8::try_from(y * 9).unwrap(), 128])
    }))
}

fn encoded(format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    picture().write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn contains(data: &[u8], wanted: &[u8]) -> bool {
    data.windows(wanted.len()).any(|window| window == wanted)
}

fn pixels(data: &[u8]) -> Vec<u8> {
    image::load_from_memory(data).unwrap().to_rgb8().into_raw()
}

fn jpeg_segment(kind: u8, body: &[u8]) -> Vec<u8> {
    let mut segment = vec![0xff, kind];
    segment.extend_from_slice(&u16::try_from(body.len() + 2).unwrap().to_be_bytes());
    segment.extend_from_slice(body);
    segment
}

/// A block as a camera writes it: little endian, with something beside the orientation.
fn camera_exif(orientation: u16) -> Vec<u8> {
    let mut body = EXIF_HEADER.to_vec();
    body.extend_from_slice(b"II*\0");
    body.extend_from_slice(&8_u32.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&0x010f_u16.to_le_bytes());
    body.extend_from_slice(&2_u16.to_le_bytes());
    body.extend_from_slice(&4_u32.to_le_bytes());
    body.extend_from_slice(b"Sony");
    body.extend_from_slice(&ORIENTATION_TAG.to_le_bytes());
    body.extend_from_slice(&SHORT_TYPE.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&orientation.to_le_bytes());
    body.extend_from_slice(&[0, 0]);
    body.extend_from_slice(SECRET);
    body
}

fn jpeg_with(segments: &[Vec<u8>], after_end: &[u8]) -> Vec<u8> {
    let plain = encoded(ImageFormat::Jpeg);
    let mut data = JPEG_START.to_vec();
    for segment in segments {
        data.extend_from_slice(segment);
    }
    data.extend_from_slice(plain.get(2..).unwrap());
    data.extend_from_slice(after_end);
    data
}

#[test]
fn a_camera_leaves_nothing_in_a_jpeg_and_the_picture_stays() {
    let data = jpeg_with(
        &[jpeg_segment(0xe1, &camera_exif(1)), jpeg_segment(0xe1, b"http://ns.adobe.com/xap/1.0/ Secret")],
        &[],
    );
    let before = pixels(&data);
    let clean = without_metadata(data);
    assert!(!contains(&clean, b"Secret") && !contains(&clean, b"Sony"));
    assert_eq!(pixels(&clean), before);
}

#[test]
fn comments_and_other_blocks_leave_a_jpeg_too() {
    let clean = without_metadata(jpeg_with(
        &[jpeg_segment(0xfe, SECRET), jpeg_segment(0xed, SECRET), jpeg_segment(0xe5, SECRET)],
        &[],
    ));
    assert!(!contains(&clean, b"Secret"));
}

#[test]
fn what_follows_a_jpeg_is_cut_off() {
    let clean = without_metadata(jpeg_with(&[], b"ftypmp42 a motion photo video Secret"));
    assert!(!contains(&clean, b"Secret"));
    assert!(clean.ends_with(&[0xff, 0xd9]));
}

#[test]
fn a_photo_stays_the_way_up_it_was_taken() {
    let clean = without_metadata(jpeg_with(&[jpeg_segment(0xe1, &camera_exif(6))], &[]));
    assert!(!contains(&clean, b"Sony") && !contains(&clean, b"Secret"));
    let block = jpeg_segment(0xe1, orientation_block(6).get(4..).unwrap());
    assert!(contains(&clean, &block));
    pixels(&clean);
}

#[test]
fn an_upright_photo_needs_no_block_at_all() {
    let clean = without_metadata(jpeg_with(&[jpeg_segment(0xe1, &camera_exif(1))], &[]));
    assert!(!contains(&clean, EXIF_HEADER));
}

#[test]
fn colour_blocks_stay_in_a_jpeg() {
    let icc = jpeg_segment(0xe2, b"ICC_PROFILE\0\x01\x01profile");
    let adobe = jpeg_segment(0xee, b"Adobe\0\x64\0\0\0\0\x01");
    let clean = without_metadata(jpeg_with(&[icc.clone(), adobe.clone()], &[]));
    assert!(contains(&clean, &icc) && contains(&clean, &adobe));
}

fn png_chunk(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
    let mut chunk = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
    chunk.extend_from_slice(&kind);
    chunk.extend_from_slice(body);
    chunk.extend_from_slice(&[0, 0, 0, 0]);
    chunk
}

#[test]
fn text_leaves_a_png_and_the_picture_stays() {
    let plain = encoded(ImageFormat::Png);
    let (head, rest) = plain.split_at(33);
    let mut data = head.to_vec();
    for kind in [b"tEXt", b"iTXt", b"zTXt", b"eXIf", b"tIME"] {
        data.extend_from_slice(&png_chunk(*kind, SECRET));
    }
    data.extend_from_slice(rest);
    data.extend_from_slice(SECRET);
    let clean = without_metadata(data);
    assert!(!contains(&clean, b"Secret"));
    assert_eq!(clean, plain);
}

fn webp_chunk(kind: [u8; 4], body: &[u8]) -> Vec<u8> {
    let mut chunk = kind.to_vec();
    chunk.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
    chunk.extend_from_slice(body);
    if body.len() % 2 == 1 {
        chunk.push(0);
    }
    chunk
}

#[test]
fn metadata_leaves_a_webp_and_the_picture_stays() {
    let plain = encoded(ImageFormat::WebP);
    let image = plain.get(WEBP_HEADER..).unwrap();
    let mut extended = vec![WEBP_METADATA_FLAGS, 0, 0, 0];
    extended.extend_from_slice(&[31, 0, 0, 23, 0, 0]);
    let mut body = webp_chunk(*b"VP8X", &extended);
    body.extend_from_slice(image);
    body.extend_from_slice(&webp_chunk(*b"EXIF", SECRET));
    body.extend_from_slice(&webp_chunk(*b"XMP ", SECRET));
    let mut data = b"RIFF".to_vec();
    data.extend_from_slice(&u32::try_from(body.len() + 4).unwrap().to_le_bytes());
    data.extend_from_slice(b"WEBP");
    data.extend_from_slice(&body);
    data.extend_from_slice(SECRET);
    let clean = without_metadata(data);
    assert!(!contains(&clean, b"Secret") && !contains(&clean, b"EXIF"));
    assert_eq!(*clean.get(WEBP_HEADER + 8).unwrap() & WEBP_METADATA_FLAGS, 0);
    assert_eq!(little_u32(&clean, 4).map(|size| size as usize + 8), Some(clean.len()));
    assert_eq!(pixels(&clean), pixels(&plain));
}

#[test]
fn a_file_that_is_no_picture_stays_as_it_is() {
    assert_eq!(without_metadata(b"plain words".to_vec()), b"plain words");
    assert_eq!(without_metadata(Vec::new()), Vec::<u8>::new());
}

#[test]
fn a_broken_picture_stays_as_it_is() {
    let mut cut = jpeg_with(&[jpeg_segment(0xe1, SECRET)], &[]);
    cut.truncate(20);
    assert_eq!(without_metadata(cut.clone()), cut);
    let mut png = PNG_START.to_vec();
    png.extend_from_slice(&[0xff, 0xff, 0xff, 0xff, b't', b'E', b'X', b't']);
    assert_eq!(without_metadata(png.clone()), png);
    let webp = b"RIFF\xff\xff\xff\xffWEBPVP8 \xff\xff\xff\xff".to_vec();
    assert_eq!(without_metadata(webp.clone()), webp);
}

#[test]
fn nonsense_at_any_length_does_no_harm() {
    let jpeg = jpeg_with(&[jpeg_segment(0xe1, &camera_exif(6))], &[]);
    // Only that nothing panics: what comes out of nonsense does not matter.
    for length in 0..jpeg.len() {
        let _ = without_metadata(jpeg.get(..length).unwrap().to_vec());
    }
    for bad in [0_u8, 0xff, 0x7f] {
        for at in 2..jpeg.len().min(200) {
            let mut broken = jpeg.clone();
            *broken.get_mut(at).unwrap() = bad;
            let _ = without_metadata(broken);
        }
    }
}
