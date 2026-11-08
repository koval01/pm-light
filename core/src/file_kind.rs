//! What a file is, judged twice.
//!
//! Before it is downloaded there are only its name and the type its sender gave, which are good for an icon and
//! nothing else. After that its bytes decide whether it is shown at all, and how: a sender's word never reaches a
//! viewer. This is no virus scanner. It keeps programs out of the viewer and says when a file is not what it
//! claims. What keeps a hostile picture or video from doing harm is that it is drawn only inside a web engine's own
//! sandboxed process, with scripts and the network off.

use crate::safety;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, imageops::FilterType};
use serde::Serialize;
use std::io::Cursor;

const BY_NAME: &[(FileKind, &[&str])] = &[
    (FileKind::Picture, &["avif", "bmp", "gif", "heic", "heif", "jpeg", "jpg", "png", "svg", "tif", "tiff", "webp"]),
    (FileKind::Video, &["3gp", "avi", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "webm", "wmv"]),
    (FileKind::Audio, &["aac", "aiff", "flac", "m4a", "mid", "midi", "mp3", "oga", "ogg", "opus", "wav", "wma"]),
    (FileKind::Pdf, &["pdf"]),
    (FileKind::Text, &[
        "c", "conf", "cpp", "css", "csv", "go", "h", "htm", "html", "ini", "java", "js", "json", "kt", "log", "md",
        "py", "rb", "rs", "sh", "sql", "swift", "toml", "ts", "tsv", "txt", "xml", "yaml", "yml",
    ]),
    (FileKind::Document, &["doc", "docm", "docx", "epub", "odt", "pages", "rtf"]),
    (FileKind::Sheet, &["numbers", "ods", "xls", "xlsm", "xlsx"]),
    (FileKind::Slides, &["key", "odp", "ppt", "pptm", "pptx"]),
    (FileKind::Archive, &["7z", "bz2", "gz", "rar", "tar", "tgz", "xz", "zip", "zst"]),
];

/// What this viewer can draw, by the type found in the bytes. Every engine in use has these.
const PICTURES: &[&str] = &["image/bmp", "image/gif", "image/jpeg", "image/png", "image/webp"];
const VIDEOS: &[&str] = &["video/mp4", "video/quicktime", "video/webm", "video/x-m4v"];
const SOUNDS: &[(&str, &str)] = &[
    ("audio/aac", "audio/aac"),
    ("audio/m4a", "audio/mp4"),
    ("audio/mpeg", "audio/mpeg"),
    ("audio/x-flac", "audio/flac"),
    ("audio/x-wav", "audio/wav"),
];
const SVG: &str = "image/svg+xml";
const PDF: &str = "application/pdf";
/// A NUL this early means it is not text, whatever it is called.
const TEXT_SAMPLE: usize = 8192;

pub const THUMBNAIL_SIDE: u32 = 96;
/// A picture larger than this keeps its icon: a thumbnail is not worth such a download.
pub const THUMBNAIL_MOST_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    Picture,
    Video,
    Audio,
    Pdf,
    Text,
    Document,
    Sheet,
    Slides,
    Archive,
    Program,
    Other,
}

/// What the viewer gets, with the type that is served for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    Picture(&'static str),
    Video(&'static str),
    Audio(&'static str),
    Pdf,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    /// It is a program, whatever it calls itself.
    Program,
    /// Its bytes are of another kind than its name.
    NotWhatItClaims,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Examined {
    pub shown: Option<Shown>,
    pub warning: Option<Warning>,
}

fn extension(name: &str) -> String {
    name.rsplit('.').next().filter(|end| end.len() < name.len()).unwrap_or_default().to_ascii_lowercase()
}

/// For an icon, and for whether a click tries the viewer. Proves nothing about the file.
#[must_use]
pub fn by_name(name: &str, mime: &str) -> FileKind {
    let end = extension(name);
    if let Some((kind, _)) = BY_NAME.iter().find(|(_, ends)| ends.contains(&end.as_str())) {
        return *kind;
    }
    if safety::is_runnable(name) {
        return FileKind::Program;
    }
    let mime = mime.to_ascii_lowercase();
    match mime.split('/').next().unwrap_or_default() {
        "image" => FileKind::Picture,
        "video" => FileKind::Video,
        "audio" => FileKind::Audio,
        "text" => FileKind::Text,
        _ if mime == PDF => FileKind::Pdf,
        _ => FileKind::Other,
    }
}

impl FileKind {
    /// For the window's icons, which are chosen by this number.
    #[must_use]
    pub const fn number(self) -> i32 {
        match self {
            Self::Other => 0,
            Self::Picture => 1,
            Self::Video => 2,
            Self::Audio => 3,
            Self::Pdf => 4,
            Self::Text => 5,
            Self::Document => 6,
            Self::Sheet => 7,
            Self::Slides => 8,
            Self::Archive => 9,
            Self::Program => 10,
        }
    }
}

/// Whether a click should try the viewer before anything is downloaded.
#[must_use]
pub const fn may_be_viewable(kind: FileKind) -> bool {
    matches!(kind, FileKind::Picture | FileKind::Video | FileKind::Audio | FileKind::Pdf | FileKind::Text)
}

fn looks_like_text(data: &[u8]) -> bool {
    !data.iter().take(TEXT_SAMPLE).any(|byte| *byte == 0)
}

fn found_in(data: &[u8]) -> Option<(FileKind, Option<Shown>)> {
    let found = infer::get(data)?;
    let mime = found.mime_type();
    let known = |list: &[&'static str]| list.iter().find(|known| **known == mime).copied();
    Some(match found.matcher_type() {
        infer::MatcherType::App => (FileKind::Program, None),
        infer::MatcherType::Image => (FileKind::Picture, known(PICTURES).map(Shown::Picture)),
        infer::MatcherType::Video => (FileKind::Video, known(VIDEOS).map(Shown::Video)),
        infer::MatcherType::Audio => {
            (FileKind::Audio, SOUNDS.iter().find(|(known, _)| *known == mime).map(|(_, served)| Shown::Audio(served)))
        }
        infer::MatcherType::Text => (FileKind::Text, Some(Shown::Text)),
        infer::MatcherType::Archive | infer::MatcherType::Doc if mime == PDF => (FileKind::Pdf, Some(Shown::Pdf)),
        infer::MatcherType::Archive => (FileKind::Archive, None),
        _ => (FileKind::Other, None),
    })
}

/// The bytes decide. A name that says otherwise earns a warning, and a program is never shown.
#[must_use]
pub fn examine(name: &str, mime: &str, data: &[u8]) -> Examined {
    let claimed = by_name(name, mime);
    let Some((kind, shown)) = found_in(data) else {
        let svg = claimed == FileKind::Picture && (extension(name) == "svg" || mime.eq_ignore_ascii_case(SVG));
        if svg && looks_like_text(data) {
            let fits = safety::is_displayable_image(SVG, data);
            return Examined { shown: fits.then_some(Shown::Picture(SVG)), warning: None };
        }
        let text = looks_like_text(data);
        let shown = text.then_some(Shown::Text).filter(|_| matches!(claimed, FileKind::Text | FileKind::Other));
        let lies = !text || may_be_viewable(claimed) && claimed != FileKind::Text;
        let warning = (lies && may_be_viewable(claimed)).then_some(Warning::NotWhatItClaims);
        return Examined { shown, warning };
    };
    if kind == FileKind::Program {
        return Examined { shown: None, warning: Some(Warning::Program) };
    }
    let picture_fits = match shown {
        Some(Shown::Picture(mime)) => safety::is_displayable_image(mime, data),
        _ => true,
    };
    let lies = may_be_viewable(claimed) && claimed != kind;
    Examined { shown: shown.filter(|_| picture_fits), warning: lies.then_some(Warning::NotWhatItClaims) }
}

fn format_of(mime: &str) -> Option<ImageFormat> {
    ImageFormat::from_mime_type(mime)
}

/// A PNG small enough to sit on a file's row, the way up the picture was taken. Decoded here, in
/// memory-safe code on a worker, and only after its size has been checked by its header.
#[must_use]
pub fn thumbnail(name: &str, mime: &str, data: &[u8]) -> Option<Vec<u8>> {
    let Some(Shown::Picture(found)) = examine(name, mime, data).shown else { return None };
    let format = format_of(found)?;
    let mut reader = ImageReader::with_format(Cursor::new(data), format).into_decoder().ok()?;
    let orientation = reader.orientation().ok()?;
    let mut picture = DynamicImage::from_decoder(reader).ok()?;
    picture.apply_orientation(orientation);
    let small = picture.resize_to_fill(THUMBNAIL_SIDE, THUMBNAIL_SIDE, FilterType::Triangle);
    let mut encoded = Cursor::new(Vec::new());
    small.to_rgba8().write_to(&mut encoded, ImageFormat::Png).ok()?;
    Some(encoded.into_inner())
}

#[cfg(test)]
mod tests;
