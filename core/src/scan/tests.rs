use super::*;
use image::Luma;
use qrcode::{EcLevel, QrCode};

/// A code drawn at scan resolution, on a white sheet.
fn sheet(text: &str, pixels: u32, margin: u32) -> GrayImage {
    let code = QrCode::with_error_correction_level(text, EcLevel::M).unwrap();
    let width = u32::try_from(code.width()).unwrap();
    let side = width * pixels + margin * 2;
    let mut image = GrayImage::from_pixel(side, side, Luma([255]));
    for (index, colour) in code.to_colors().iter().enumerate() {
        if *colour == qrcode::Color::Dark {
            let (x, y) = (u32::try_from(index).unwrap() % width, u32::try_from(index).unwrap() / width);
            for dx in 0..pixels {
                for dy in 0..pixels {
                    image.put_pixel(margin + x * pixels + dx, margin + y * pixels + dy, Luma([0]));
                }
            }
        }
    }
    image
}

fn read(image: GrayImage) -> Vec<String> {
    codes_in(&DynamicImage::ImageLuma8(image))
}

#[test]
fn reads_a_clean_code() {
    assert_eq!(read(sheet("PMS1.hello", 6, 60)), ["PMS1.hello"]);
}

#[test]
fn reads_a_sheet_scanned_upside_down_or_sideways() {
    let clean = sheet("PMS1.hello", 6, 60);
    assert_eq!(read(imageops::rotate90(&clean)), ["PMS1.hello"]);
    assert_eq!(read(imageops::rotate180(&clean)), ["PMS1.hello"]);
}

#[test]
fn reads_a_sheet_laid_slightly_crooked() {
    let clean = sheet("PMS1.hello", 8, 120);
    let turned = imageproc_free_rotate(&clean, 4.0);
    assert_eq!(read(turned), ["PMS1.hello"]);
}

/// Enough of a rotation for a test, with no library for it.
fn imageproc_free_rotate(image: &GrayImage, degrees: f64) -> GrayImage {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (w, h) = (f64::from(image.width()), f64::from(image.height()));
    GrayImage::from_fn(image.width(), image.height(), |x, y| {
        let (dx, dy) = (f64::from(x) - w / 2.0, f64::from(y) - h / 2.0);
        let (sx, sy) = (f64::mul_add(sin, dy, cos * dx) + w / 2.0, f64::mul_add(sin, -dx, cos * dy) + h / 2.0);
        let inside = sx >= 0.0 && sy >= 0.0 && sx < w && sy < h;
        // Inside means both are between zero and the side, so they fit a pixel index.
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "checked to be inside the picture")]
        let at = (sx as u32, sy as u32);
        if inside { *image.get_pixel(at.0, at.1) } else { Luma([255]) }
    })
}

#[test]
fn reads_a_code_as_dense_as_a_signed_statement() {
    let dense = format!("-----BEGIN PGP SIGNED MESSAGE-----\n{}", "Signed text of a letter. ".repeat(28));
    assert_eq!(read(sheet(&dense, 4, 80)), [dense]);
}

#[test]
fn the_same_code_seen_twice_counts_once() {
    assert_eq!(read(sheet("PMS1.once", 12, 120)).len(), 1);
}

#[test]
fn a_sheet_with_no_code_has_none() {
    assert!(read(GrayImage::from_pixel(200, 200, Luma([255]))).is_empty());
}

#[test]
fn a_file_that_is_no_image_is_refused_and_does_not_panic() {
    for junk in [&b""[..], b"not an image", &[0x89, b'P', b'N', b'G'], &[0xff, 0xd8, 0xff]] {
        assert!(matches!(read_sheets(junk), Err(Error::UnreadableScan)));
    }
}

#[test]
fn a_picture_claiming_to_be_vast_is_refused_before_it_is_decoded() {
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&100_000_u32.to_be_bytes());
    png.extend_from_slice(&100_000_u32.to_be_bytes());
    png.extend_from_slice(&[8, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(matches!(read_sheets(&png), Err(Error::UnreadableScan)));
}

/// A band of faded toner, or a stain: what is under it is lost, not merely hard to see.
fn wiped(mut image: GrayImage, from: u32, to: u32) -> GrayImage {
    for x in from..to {
        for y in 0..image.height() {
            image.put_pixel(x, y, Luma([255]));
        }
    }
    image
}

fn sheet_at(text: &str, level: EcLevel, pixels: u32) -> GrayImage {
    let code = QrCode::with_error_correction_level(text, level).unwrap();
    let width = u32::try_from(code.width()).unwrap();
    let margin = 8 * pixels;
    let side = width * pixels + margin * 2;
    let mut image = GrayImage::from_pixel(side, side, Luma([255]));
    for (index, colour) in code.to_colors().iter().enumerate() {
        if *colour == qrcode::Color::Dark {
            let (x, y) = (u32::try_from(index).unwrap() % width, u32::try_from(index).unwrap() / width);
            for dx in 0..pixels {
                for dy in 0..pixels {
                    image.put_pixel(margin + x * pixels + dx, margin + y * pixels + dy, Luma([0]));
                }
            }
        }
    }
    image
}

#[test]
fn the_new_codes_survive_a_band_of_lost_toner() {
    let text = format!("PMS1.a.0.1.{}", "abcdefghij".repeat(47));
    let image = sheet_at(&text, EcLevel::Q, 5);
    let side = image.width();
    assert_eq!(read(wiped(image, side * 40 / 100, side * 50 / 100)), [text]);
}

#[test]
fn a_scanned_pdf_gives_its_pages() {
    let text = "PMS1.pdf";
    let mut jpeg = Cursor::new(Vec::new());
    DynamicImage::ImageLuma8(sheet(text, 8, 80)).write_to(&mut jpeg, ImageFormat::Jpeg).unwrap();
    let jpeg = jpeg.into_inner();
    let mut pdf = b"%PDF-1.3\n4 0 obj\n<< /Subtype /Image /Filter /DCTDecode >>\nstream\n".to_vec();
    pdf.extend_from_slice(&jpeg);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let (codes, pictures) = read_sheets(&pdf).unwrap();
    assert_eq!(codes, [text]);
    assert_eq!(pictures.len(), 1);
}

#[test]
fn a_pdf_with_nothing_to_read_is_refused_and_junk_cannot_panic() {
    for junk in [
        &b"%PDF-1.3"[..],
        b"%PDF-1.3 stream",
        b"%PDF obj /Image stream\n",
        b"%PDF obj /Image /FlateDecode /Width 99999 stream\nendstream",
    ] {
        assert!(matches!(read_sheets(junk), Err(Error::UnreadableScan)));
    }
}
