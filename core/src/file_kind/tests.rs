use super::*;
use image::{ImageFormat, RgbImage};

const WINDOWS_PROGRAM: &[u8] = b"MZ\x90\x00\x03\x00\x00\x00\x04\x00\x00\x00\xff\xff";
const PDF_FILE: &[u8] = b"%PDF-1.7\n1 0 obj\n<<>>\nendobj\n";

fn encoded(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
    let picture = RgbImage::from_fn(width, height, |x, _| image::Rgb([u8::try_from(x % 255).unwrap(), 10, 200]));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(picture).write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn mp4() -> Vec<u8> {
    let mut data = vec![0, 0, 0, 0x18];
    data.extend_from_slice(b"ftypmp42\0\0\0\0mp42isom");
    data.resize(64, 0);
    data
}

#[test]
fn a_name_is_enough_for_an_icon() {
    assert_eq!(by_name("photo.JPG", ""), FileKind::Picture);
    assert_eq!(by_name("clip.mov", ""), FileKind::Video);
    assert_eq!(by_name("song.flac", ""), FileKind::Audio);
    assert_eq!(by_name("paper.pdf", ""), FileKind::Pdf);
    assert_eq!(by_name("report.docx", ""), FileKind::Document);
    assert_eq!(by_name("table.xlsx", ""), FileKind::Sheet);
    assert_eq!(by_name("talk.pptx", ""), FileKind::Slides);
    assert_eq!(by_name("backup.zip", ""), FileKind::Archive);
    assert_eq!(by_name("setup.exe", ""), FileKind::Program);
    assert_eq!(by_name("unknown.xyz", ""), FileKind::Other);
    assert_eq!(by_name("noname", "image/png"), FileKind::Picture);
    assert_eq!(by_name("noname", "application/pdf"), FileKind::Pdf);
}

#[test]
fn a_picture_is_served_as_what_it_is() {
    for (format, mime) in
        [(ImageFormat::Png, "image/png"), (ImageFormat::Jpeg, "image/jpeg"), (ImageFormat::WebP, "image/webp")]
    {
        let found = examine("a.dat", "", &encoded(format, 4, 4));
        assert_eq!(found.shown, Some(Shown::Picture(mime)));
    }
}

#[test]
fn a_sender_s_word_is_not_what_is_served() {
    let found = examine("photo.png", "text/html", &encoded(ImageFormat::Jpeg, 4, 4));
    assert_eq!(found.shown, Some(Shown::Picture("image/jpeg")));
    assert_eq!(found.warning, None);
}

#[test]
fn a_program_is_never_shown_whatever_it_is_called() {
    for name in ["holiday.jpg", "report.pdf", "notes.txt", "setup.exe", "plain"] {
        let found = examine(name, "image/jpeg", WINDOWS_PROGRAM);
        assert_eq!(found, Examined { shown: None, warning: Some(Warning::Program) }, "{name}");
    }
}

#[test]
fn a_file_that_is_not_what_it_claims_is_said_to_be_so() {
    let pretending = examine("invoice.pdf", "application/pdf", b"just some words");
    assert_eq!(pretending.warning, Some(Warning::NotWhatItClaims));
    assert_eq!(pretending.shown, None);
    let picture_as_pdf = examine("invoice.pdf", "", &encoded(ImageFormat::Png, 4, 4));
    assert_eq!(picture_as_pdf.warning, Some(Warning::NotWhatItClaims));
    assert_eq!(picture_as_pdf.shown, Some(Shown::Picture("image/png")));
}

#[test]
fn media_and_documents_are_found_by_their_bytes() {
    assert_eq!(examine("clip.mp4", "", &mp4()).shown, Some(Shown::Video("video/mp4")));
    assert_eq!(examine("paper.pdf", "", PDF_FILE), Examined { shown: Some(Shown::Pdf), warning: None });
    let mp3 = [b"ID3\x03\0\0\0\0\0\0".as_slice(), &[0; 32]].concat();
    assert_eq!(examine("song.mp3", "", &mp3).shown, Some(Shown::Audio("audio/mpeg")));
}

#[test]
fn text_is_text_only_while_it_looks_like_it() {
    assert_eq!(examine("notes.txt", "text/plain", b"plain words").shown, Some(Shown::Text));
    assert_eq!(examine("notes.csv", "", b"a,b\n1,2\n").shown, Some(Shown::Text));
    let binary = examine("notes.txt", "text/plain", b"bin\0ary");
    assert_eq!((binary.shown, binary.warning), (None, Some(Warning::NotWhatItClaims)));
}

#[test]
fn a_page_or_a_script_is_read_and_never_run() {
    let page = examine("page.html", "text/html", b"<!doctype html><html><script>alert(1)</script></html>");
    assert_eq!(page.shown, Some(Shown::Text));
}

#[test]
fn an_svg_needs_its_name_or_its_type() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
    assert_eq!(examine("a.svg", "", svg).shown, Some(Shown::Picture(SVG)));
    assert_eq!(examine("a.txt", "", svg).shown, Some(Shown::Text));
}

#[test]
fn what_the_viewer_cannot_draw_is_left_alone() {
    let zip = b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    assert_eq!(examine("backup.zip", "", zip), Examined { shown: None, warning: None });
}

#[test]
fn a_thumbnail_is_small_and_square_and_a_program_has_none() {
    let thumb = thumbnail("a.png", "", &encoded(ImageFormat::Png, 300, 100)).unwrap();
    let decoded = image::load_from_memory(&thumb).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (THUMBNAIL_SIDE, THUMBNAIL_SIDE));
    assert_eq!(thumbnail("a.png", "", WINDOWS_PROGRAM), None);
    assert_eq!(thumbnail("a.png", "", b"plain words"), None);
}

#[test]
fn a_thumbnail_comes_out_the_way_up_it_was_taken() {
    let turned = crate::image_metadata::tests_support::turned_jpeg(&encoded(ImageFormat::Jpeg, 300, 100), 6);
    let thumb = thumbnail("a.jpg", "", &turned).unwrap();
    assert!(image::load_from_memory(&thumb).is_ok());
}
