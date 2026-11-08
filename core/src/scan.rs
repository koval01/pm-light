//! Reads check codes from a scan or a photo. The file is hostile until proven otherwise: its size is
//! limited before it is decoded, and this runs on a worker, where a panic costs one scan.

use crate::error::{Error, Result};
use flate2::read::ZlibDecoder;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{
    DynamicImage, GrayImage, ImageFormat, ImageReader, Limits, RgbImage,
    imageops::{self, FilterType},
};
use rxing::{BarcodeFormat, DecodeHints};
use std::{collections::HashSet, io::{Cursor, Read}};

/// A sheet at 600 dpi fits, with room to spare.
const MOST_PIXELS_ACROSS: u32 = 12_000;
const MOST_MEMORY: u64 = 512 * 1024 * 1024;
const PREVIEW_WIDTH: u32 = 1100;
const MAX_CODE_BYTES: usize = 4096;
const SCALES: [u32; 5] = [100, 66, 50, 40, 33];
const PARTS: [u32; 2] = [2, 3];
/// Looking at every fourth pixel finds blocks as well as looking at all of them.
const SAMPLE: u32 = 4;
const INK: u8 = 128;
/// In parts of the sheet's width: white this wide parts two blocks, and a block is no narrower
/// than the other. About 2 mm and 18 mm on A4.
const GAP_PARTS: u32 = 100;
const LEAST_BLOCK_PARTS: u32 = 12;
const MOST_PDF_PAGES: usize = 64;
const PDF_DICTIONARY_BYTES: usize = 4096;

fn decoded(bytes: &[u8]) -> Result<DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(Error::other_from)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MOST_PIXELS_ACROSS);
    limits.max_image_height = Some(MOST_PIXELS_ACROSS);
    limits.max_alloc = Some(MOST_MEMORY);
    reader.limits(limits);
    reader.decode().map_err(|_| Error::UnreadableScan)
}

fn codes_at(image: GrayImage) -> Vec<String> {
    let mut prepared = rqrr::PreparedImage::prepare(image);
    prepared
        .detect_grids()
        .iter()
        .filter_map(|grid| {
            let mut bytes = Vec::new();
            grid.decode_to(&mut bytes).ok()?;
            (bytes.len() <= MAX_CODE_BYTES).then(|| String::from_utf8(bytes).ok())?
        })
        .collect()
}

fn keep(found: &mut Vec<String>, codes: Vec<String>) {
    for code in codes {
        if code.len() <= MAX_CODE_BYTES && !found.contains(&code) {
            found.push(code);
        }
    }
}

/// Finds codes `codes_at` misses on worn sheets, and is the stronger of the two on stains and fading.
fn codes_by_pattern(picture: &GrayImage, hints: &mut DecodeHints) -> Vec<String> {
    let (width, height) = picture.dimensions();
    rxing::helpers::detect_multiple_in_luma_with_hints(picture.as_raw().clone(), width, height, hints)
        .map(|found| found.iter().map(|code| code.getText().to_string()).collect())
        .unwrap_or_default()
}

#[derive(Clone, Copy)]
struct Area {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Stretches along one direction that hold ink, ignoring gaps too narrow to part two things.
fn ink_runs(profile: &[u32], gap: usize) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (at, ink) in profile.iter().enumerate() {
        if *ink == 0 {
            continue;
        }
        match runs.last_mut() {
            Some((_, end)) if at - *end <= gap => *end = at,
            _ => runs.push((at, at)),
        }
    }
    runs
}

fn split(full: &GrayImage, area: Area, across: bool, gap: u32) -> Vec<Area> {
    let (length, other) = if across { (area.width, area.height) } else { (area.height, area.width) };
    let mut profile = vec![0; (length / SAMPLE) as usize + 1];
    for along in (0..length).step_by(SAMPLE as usize) {
        let mut ink = 0;
        for side in (0..other).step_by(SAMPLE as usize) {
            let (x, y) = if across { (along, side) } else { (side, along) };
            if full.get_pixel_checked(area.x + x, area.y + y).is_some_and(|pixel| pixel.0[0] < INK) {
                ink += 1;
            }
        }
        // A speck of dust on the scanner is not a thing on the sheet.
        if let Some(slot) = profile.get_mut((along / SAMPLE) as usize) {
            *slot = if ink > 2 { ink } else { 0 };
        }
    }
    ink_runs(&profile, (gap / SAMPLE) as usize)
        .into_iter()
        .filter_map(|(start, end)| {
            let (start, end) = (u32::try_from(start).ok()? * SAMPLE, u32::try_from(end).ok()? * SAMPLE + SAMPLE);
            let size = end.min(length).checked_sub(start)?;
            Some(if across {
                Area { x: area.x + start, width: size, ..area }
            } else {
                Area { y: area.y + start, height: size, ..area }
            })
        })
        .collect()
}

/// The square blocks on a sheet, cut apart along the white between them: a code alone in a picture
/// is read where one among its neighbours is not, and no layout has to be known for this.
fn blocks(full: &GrayImage) -> Vec<Area> {
    let gap = full.width() / GAP_PARTS;
    let least = full.width() / LEAST_BLOCK_PARTS;
    let mut areas = vec![Area { x: 0, y: 0, width: full.width(), height: full.height() }];
    for across in [false, true, false, true] {
        areas = areas.into_iter().flat_map(|area| split(full, area, across, gap)).collect();
    }
    areas.retain(|area| {
        let (short, long) = (area.width.min(area.height), area.width.max(area.height));
        short >= least && long <= short * 2
    });
    areas
}

/// Each reader has sizes it misses and damage it survives, so a sheet is read by both, whole at
/// several sizes, then in overlapping parts: one code alone in a picture is easier to lock on to.
fn codes_in(image: &DynamicImage) -> Vec<String> {
    let full = image.to_luma8();
    let mut hints = DecodeHints {
        TryHarder: Some(true),
        PossibleFormats: Some(HashSet::from([BarcodeFormat::QR_CODE])),
        ..DecodeHints::default()
    };
    let mut found = Vec::new();
    for percent in SCALES {
        let (width, height) = (full.width() * percent / 100, full.height() * percent / 100);
        let scaled = if percent == 100 {
            full.clone()
        } else {
            imageops::resize(&full, width, height, FilterType::Triangle)
        };
        keep(&mut found, codes_by_pattern(&scaled, &mut hints));
        keep(&mut found, codes_at(scaled));
    }
    for area in blocks(&full) {
        let margin = full.width() / GAP_PARTS;
        let (x, y) = (area.x.saturating_sub(margin), area.y.saturating_sub(margin));
        let block = imageops::crop_imm(&full, x, y, area.width + margin * 2, area.height + margin * 2).to_image();
        keep(&mut found, codes_by_pattern(&block, &mut hints));
        keep(&mut found, codes_at(block));
    }
    let (width, height) = full.dimensions();
    for parts in PARTS {
        let (part_width, part_height) = (width / parts, height / parts);
        for row in 0..parts * 2 - 1 {
            for column in 0..parts * 2 - 1 {
                let (x, y) = (column * part_width / 2, row * part_height / 2);
                let part = imageops::crop_imm(&full, x, y, part_width, part_height).to_image();
                keep(&mut found, codes_by_pattern(&part, &mut hints));
            }
        }
    }
    found
}

fn preview(image: &DynamicImage) -> Option<String> {
    let small = image.resize(PREVIEW_WIDTH, PREVIEW_WIDTH * 2, FilterType::Triangle).to_rgb8();
    let mut bytes = Cursor::new(Vec::new());
    small.write_to(&mut bytes, ImageFormat::Jpeg).ok()?;
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes.into_inner())))
}

fn find(bytes: &[u8], wanted: &[u8], from: usize) -> Option<usize> {
    let rest = bytes.get(from..)?;
    rest.windows(wanted.len()).position(|part| part == wanted).map(|at| at + from)
}

fn contains(bytes: &[u8], wanted: &[u8]) -> bool {
    find(bytes, wanted, 0).is_some()
}

/// The number after a name in a PDF dictionary.
fn number_after(dictionary: &[u8], name: &[u8]) -> Option<u32> {
    let after = dictionary.get(find(dictionary, name, 0)? + name.len()..)?;
    let digits: Vec<u8> =
        after.iter().copied().skip_while(u8::is_ascii_whitespace).take_while(u8::is_ascii_digit).collect();
    String::from_utf8(digits).ok()?.parse().ok()
}

/// Flate pictures are unpacked only as far as their size says they can reach.
fn flate_page(dictionary: &[u8], data: &[u8]) -> Option<DynamicImage> {
    let (width, height) = (number_after(dictionary, b"/Width")?, number_after(dictionary, b"/Height")?);
    if width > MOST_PIXELS_ACROSS || height > MOST_PIXELS_ACROSS {
        return None;
    }
    let colours: u32 = if contains(dictionary, b"/DeviceGray") { 1 } else { 3 };
    if number_after(dictionary, b"/BitsPerComponent") != Some(8) || contains(dictionary, b"/Predictor") {
        return None;
    }
    let size = u64::from(width) * u64::from(height) * u64::from(colours);
    if size > MOST_MEMORY {
        return None;
    }
    let mut pixels = Vec::new();
    ZlibDecoder::new(data).take(size).read_to_end(&mut pixels).ok()?;
    if colours == 1 {
        GrayImage::from_raw(width, height, pixels).map(DynamicImage::ImageLuma8)
    } else {
        RgbImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgb8)
    }
}

/// The pictures in a scanned PDF. A scanner writes one picture for each page, so this reads those
/// and draws nothing: a PDF made of text and shapes has no pictures, and stays unreadable here.
fn pdf_pages(bytes: &[u8]) -> Vec<DynamicImage> {
    let (mut pages, mut at) = (Vec::new(), 0);
    while pages.len() < MOST_PDF_PAGES {
        let Some(mark) = find(bytes, b"stream", at) else { break };
        at = mark + 1;
        let start = mark.saturating_sub(PDF_DICTIONARY_BYTES);
        let Some(before) = bytes.get(start..mark) else { continue };
        let opened = before.windows(3).rposition(|part| part == b"obj").unwrap_or(0);
        let Some(dictionary) = before.get(opened..) else { continue };
        if !contains(dictionary, b"/Image") {
            continue;
        }
        let mut data_start = mark + b"stream".len();
        data_start += match bytes.get(data_start..data_start + 2) {
            Some(b"\r\n") => 2,
            _ => 1,
        };
        let Some(end) = find(bytes, b"endstream", data_start) else { break };
        at = end + 1;
        let Some(data) = bytes.get(data_start..end) else { continue };
        let page = if contains(dictionary, b"/DCTDecode") {
            decoded(data).ok()
        } else if contains(dictionary, b"/FlateDecode") {
            flate_page(dictionary, data)
        } else {
            None
        };
        pages.extend(page);
    }
    pages
}

/// The codes in a file, and a picture of each sheet to put beside what they say.
pub fn read_sheets(bytes: &[u8]) -> Result<(Vec<String>, Vec<String>)> {
    let pages = if bytes.starts_with(b"%PDF") { pdf_pages(bytes) } else { vec![decoded(bytes)?] };
    if pages.is_empty() {
        return Err(Error::UnreadableScan);
    }
    let (mut codes, mut pictures) = (Vec::new(), Vec::new());
    for page in &pages {
        keep(&mut codes, codes_in(page));
        pictures.extend(preview(page));
    }
    Ok((codes, pictures))
}

#[cfg(test)]
mod tests {
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
        for junk in [&b"%PDF-1.3"[..], b"%PDF-1.3 stream", b"%PDF obj /Image stream\n", b"%PDF obj /Image /FlateDecode /Width 99999 stream\nendstream"] {
            assert!(matches!(read_sheets(junk), Err(Error::UnreadableScan)));
        }
    }
}
