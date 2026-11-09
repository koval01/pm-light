use super::*;

#[test]
fn a_measured_page_is_drawn_at_its_height() {
    assert_eq!(drawn_height("1234"), (1234.0, false));
    assert_eq!(drawn_height("\"880\""), (880.0, false));
    assert_eq!(drawn_height(" 42 \n"), (42.0, false));
}

#[test]
fn a_page_past_the_tallest_is_cut_there() {
    assert_eq!(drawn_height("90000"), (TALLEST, true));
    assert_eq!(drawn_height(&format!("{TALLEST}")), (TALLEST, false));
}

#[test]
fn nonsense_from_the_page_is_one_line() {
    for measured in ["", "null", "NaN", "Infinity", "-5", "0", "1e999", "abc", "\"\""] {
        assert_eq!(drawn_height(measured), (1.0, false), "{measured:?}");
    }
}

#[test]
fn an_ordinary_mail_is_drawn_twice_as_fine() {
    assert!((scale_for(WIDTH, 2000.0) - SCALE).abs() < f64::EPSILON);
}

#[test]
fn a_tall_mail_is_drawn_less_fine_within_its_pixels() {
    let scale = scale_for(WIDTH, TALLEST);
    assert!((1.0..SCALE).contains(&scale));
    let pixels = WIDTH * scale * TALLEST * scale;
    assert!(pixels <= MOST_PIXELS + 1.0, "{pixels}");
}

#[test]
fn a_size_without_area_takes_the_usual_scale() {
    for (width, height) in [(0.0, 10.0), (WIDTH, -3.0), (f64::NAN, 10.0), (f64::INFINITY, 10.0)] {
        assert!((scale_for(width, height) - SCALE).abs() < f64::EPSILON);
    }
}

#[test]
fn the_picture_holds_the_mail_and_nothing_for_paper() {
    let chosen = options(false);
    assert!(chosen.headers);
    assert!(!chosen.images && options(true).images);
    assert!(!chosen.check_code && !chosen.seal && !chosen.hole_mark && !chosen.technical && !chosen.history);
    assert!(!chosen.new_sheet_each);
    assert_eq!(chosen.fold_marks, proton_core::print::FoldMarks::None);
}

#[test]
fn a_shared_file_names_nothing_of_the_mail() {
    assert_eq!(SHARED_NAME, "mail.png");
}

#[cfg(target_os = "macos")]
#[test]
fn a_picture_on_a_pasteboard_reads_back_as_png_and_as_tiff() {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap_or_default();
    let (read, tiff) = macos::put_and_read_back(&png).unwrap_or_default();
    assert_eq!(read, png);
    assert!(tiff, "programs that read only TIFF get one from the pasteboard, which is why none is written");
}

/// A page of 200 by 100 points with a black square from (10, 10) to (60, 60), counted from its bottom left.
#[cfg(target_os = "macos")]
fn square_pdf() -> Vec<u8> {
    pdf_with_pages(&["0 0 200 100"])
}

/// A PDF of a page for each media box given, each with the same black square.
#[cfg(target_os = "macos")]
fn pdf_with_pages(boxes: &[&str]) -> Vec<u8> {
    let content = "0 0 0 rg 10 10 50 50 re f";
    let kids: Vec<String> = (0..boxes.len()).map(|page| format!("{} 0 R", 3 + page * 2)).collect();
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), boxes.len()),
    ];
    for (page, media) in boxes.iter().enumerate() {
        objects.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [{media}] /Contents {} 0 R >>", 4 + page * 2));
        objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
    }
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (number, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", number + 1).as_bytes());
    }
    let table = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n", objects.len() + 1).as_bytes());
    pdf
}

#[cfg(target_os = "macos")]
#[test]
fn a_pdf_page_becomes_pixels_twice_as_fine_on_white() {
    let pdf = objc2_foundation::NSData::with_bytes(&square_pdf());
    let png = macos::drawn(&pdf).unwrap_or_default();
    let picture = image::load_from_memory(&png).map(|picture| picture.to_rgba8()).unwrap_or_default();
    assert_eq!(picture.dimensions(), (400, 200), "the page's own size, twice");
    // Pixels count from the top: the square sits at the bottom left.
    assert_eq!(picture.get_pixel(70, 130).0, [0, 0, 0, 255], "inside the square");
    assert_eq!(picture.get_pixel(300, 50).0, [255, 255, 255, 255], "white, not see-through, around it");
}

#[cfg(target_os = "macos")]
#[test]
fn what_is_not_a_pdf_is_no_picture() {
    let data = objc2_foundation::NSData::with_bytes(b"<html>not a pdf</html>");
    assert!(macos::drawn(&data).is_none());
}

/// The size of a picture `drawn` made, or nothing.
#[cfg(target_os = "macos")]
fn drawn_size(pdf: &[u8]) -> Option<(u32, u32)> {
    let png = macos::drawn(&objc2_foundation::NSData::with_bytes(pdf))?;
    image::load_from_memory(&png).ok().map(|picture| (picture.width(), picture.height()))
}

/// What reaches AppKit's PDF and bitmap calls behind `unsafe`: pages of no size, of a size past any picture, upside
/// down, unreadable, cut short, nothing at all. Each must give a picture within bounds or none, never a crash, and
/// under the address sanitizer never a touch outside memory.
#[cfg(target_os = "macos")]
#[test]
fn hostile_pdfs_give_a_bounded_picture_or_none() {
    let most = (TALLEST * SCALE).round();
    let bounded = |size: Option<(u32, u32)>| size.is_none_or(|(wide, high)| f64::from(wide) <= most && f64::from(high) <= most && wide > 0 && high > 0);
    for media in ["0 0 0 0", "0 0 0 100", "0 0 200 0", "0 0 -200 -100", "200 100 0 0", "0 0 0.4 0.4", "0 0 1 1", "0 0 99999 99999",
        "0 0 1 999999", "0 0 999999 1", "0 0 abc def", "0 0", "", "1e308 1e308 -1e308 -1e308", "0 0 nan nan"] {
        assert!(bounded(drawn_size(&pdf_with_pages(&[media]))), "media box [{media}]");
    }
    let whole = square_pdf();
    for cut in [0, 1, 8, whole.len() / 3, whole.len() / 2, whole.len() - 10] {
        let part = whole.get(..cut).unwrap_or_default();
        assert!(bounded(drawn_size(part)), "a PDF cut at {cut}");
    }
    assert_eq!(drawn_size(&[]), None);
    assert_eq!(drawn_size(&[0; 4096]), None);
}

#[cfg(target_os = "macos")]
#[test]
fn of_many_pages_only_the_first_is_drawn() {
    assert_eq!(drawn_size(&pdf_with_pages(&["0 0 300 150", "0 0 50 50", "0 0 2000 2000"])), Some((600, 300)));
}

/// Hundreds of pictures one after another: the address sanitizer catches a use after free, and memory must not pile up.
#[cfg(target_os = "macos")]
#[test]
fn drawing_again_and_again_is_the_same_picture() {
    let pdf = objc2_foundation::NSData::with_bytes(&square_pdf());
    let first = macos::drawn(&pdf).unwrap_or_default();
    for _ in 0..300 {
        assert_eq!(macos::drawn(&pdf).unwrap_or_default(), first);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn a_pasteboard_takes_nothing_one_byte_and_much() {
    for size in [0_usize, 1, 3, 4096, 20 * 1024 * 1024] {
        let bytes: Vec<u8> = (0..size).map(|index| u8::try_from(index % 251).unwrap_or(0)).collect();
        let read = macos::put_and_read_back(&bytes).map(|(read, _)| read);
        assert!(read.is_none_or(|read| read == bytes), "{size} bytes came back changed");
    }
}

fn tiny_png() -> Vec<u8> {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap_or_default();
    png
}

#[test]
fn a_devtools_answer_gives_its_png() {
    use base64::Engine as _;
    let png = tiny_png();
    let answer = format!(r#"{{"data":"{}"}}"#, base64::engine::general_purpose::STANDARD.encode(&png));
    assert_eq!(png_of_devtools(&answer), Some(png));
}

#[test]
fn a_devtools_answer_without_a_png_gives_nothing() {
    use base64::Engine as _;
    let not_png = base64::engine::general_purpose::STANDARD.encode(b"<svg/>");
    for answer in ["", "{}", r#"{"data":5}"#, r#"{"data":"%%%"}"#, &format!(r#"{{"data":"{not_png}"}}"#), "[1,2]"] {
        assert_eq!(png_of_devtools(answer), None, "{answer}");
    }
}

#[test]
fn a_bitmap_for_windows_runs_from_the_bottom_row_in_blue_green_red() {
    let mut picture = image::RgbaImage::new(2, 2);
    picture.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
    picture.put_pixel(1, 1, image::Rgba([40, 50, 60, 128]));
    let bitmap = dib(&picture);
    assert_eq!(bitmap.len(), 40 + 2 * 2 * 4);
    assert_eq!(bitmap.get(0..4), Some(&40_u32.to_le_bytes()[..]));
    assert_eq!(bitmap.get(4..8), Some(&2_i32.to_le_bytes()[..]));
    assert_eq!(bitmap.get(8..12), Some(&2_i32.to_le_bytes()[..]), "positive height: bottom up");
    assert_eq!(bitmap.get(12..14), Some(&1_u16.to_le_bytes()[..]));
    assert_eq!(bitmap.get(14..16), Some(&32_u16.to_le_bytes()[..]));
    assert!(bitmap.get(16..40).is_some_and(|rest| rest.iter().all(|byte| *byte == 0)));
    // The bottom row first: its second pixel is (40, 50, 60).
    assert_eq!(bitmap.get(40..48), Some(&[0, 0, 0, 0, 60, 50, 40, 0][..]));
    // Then the top row: its first pixel is (10, 20, 30).
    assert_eq!(bitmap.get(48..52), Some(&[30, 20, 10, 0][..]));
}

#[test]
fn cairo_pixels_come_out_straight_and_in_order() {
    // One row of two pixels and a padded stride: opaque red, and half-covered white (premultiplied to 128).
    let data = [0, 0, 255, 255, 128, 128, 128, 128, 9, 9, 9, 9];
    let picture = rgba_of_argb32(&data, 2, 1, 12).unwrap_or_default();
    assert_eq!(picture.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(picture.get_pixel(1, 0).0, [255, 255, 255, 128]);
}

#[test]
fn cairo_pixels_that_do_not_fit_their_sizes_are_refused() {
    let data = [0_u8; 16];
    assert!(rgba_of_argb32(&data, 2, 2, 4).is_none(), "a stride shorter than a row");
    assert!(rgba_of_argb32(&data, 2, 3, 8).is_none(), "more rows than data");
    assert!(rgba_of_argb32(&data, -1, 2, 8).is_none());
    assert!(rgba_of_argb32(&data, 0, 2, 8).is_none());
    assert!(rgba_of_argb32(&data, 2, 2, -8).is_none());
    assert!(rgba_of_argb32(&[0, 0, 0, 0], 1, 1, 4).is_some_and(|picture| picture.get_pixel(0, 0).0 == [0, 0, 0, 0]));
}

#[test]
fn a_picture_drops_papers_margins_after_printings_own() {
    let prepared = PrintDocument { style: ".print-part{padding:0 18mm 0 22mm}".to_string(), fit: String::new(), parts: Vec::new() };
    let style = for_a_picture(prepared).style;
    let (printed, picture) = (style.find("18mm").unwrap_or(usize::MAX), style.find("24px").unwrap_or(0));
    assert!(printed < picture, "the later rule wins: {style}");
}

#[test]
fn a_full_document_is_cut_where_the_picture_ends() {
    // Drawn twice as fine: 1440 pixels across a page of 720 points, 16000 points high at most.
    assert_eq!(kept_rows(100_000, 1440, WIDTH, TALLEST), 32_000);
    assert_eq!(kept_rows(500, 1440, WIDTH, TALLEST), 500, "a short one keeps every row");
    assert_eq!(kept_rows(500, 720, WIDTH, 100.0), 100);
}

#[test]
fn sizes_that_make_no_sense_keep_every_row() {
    for (wide, across, high) in [(0, WIDTH, 10.0), (1440, 0.0, 10.0), (1440, f64::NAN, 10.0), (1440, WIDTH, f64::INFINITY)] {
        assert_eq!(kept_rows(300, wide, across, high), 300);
    }
}
