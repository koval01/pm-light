//! Takes what a camera wrote about a picture out of the file.
//!
//! That is the place, time, device, thumbnails, and whatever follows the picture, such as the video of a motion photo. Proton's
//! client draws the picture and encodes it again. This cuts the file instead, so not one pixel changes.
//!
//! A file that is not what it looks like stays as it is: this is a person's own attachment, and
//! sending it unchanged is better than sending half of it.

const JPEG_START: &[u8] = &[0xff, 0xd8];
const PNG_START: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

const MARKER: u8 = 0xff;
const MARKER_SCAN: u8 = 0xda;
const MARKER_END: u8 = 0xd9;
const MARKER_APP0: u8 = 0xe0;
const MARKER_APP1: u8 = 0xe1;
const MARKER_APP2: u8 = 0xe2;
const MARKER_APP14: u8 = 0xee;
const MARKER_APP15: u8 = 0xef;
const MARKER_COMMENT: u8 = 0xfe;
const EXIF_HEADER: &[u8] = b"Exif\0\0";
const ORIENTATION_TAG: u16 = 0x0112;
const SHORT_TYPE: u16 = 3;
const UPRIGHT: u16 = 1;
const LAST_ORIENTATION: u16 = 8;
const IFD_ENTRY: usize = 12;

/// What decoders need to draw a PNG the same way. All text and `eXIf` stay behind.
const PNG_KEPT: &[&[u8; 4]] = &[
    b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"gAMA", b"cHRM", b"sRGB", b"iCCP", b"sBIT", b"bKGD", b"pHYs",
    b"acTL", b"fcTL", b"fdAT",
];
const PNG_LAST: &[u8; 4] = b"IEND";

const WEBP_HEADER: usize = 12;
const WEBP_KEPT: &[&[u8; 4]] = &[b"VP8 ", b"VP8L", b"VP8X", b"ALPH", b"ANIM", b"ANMF", b"ICCP"];
const WEBP_EXTENDED: &[u8; 4] = b"VP8X";
/// In the first byte of `VP8X`: says that metadata chunks follow, which they no longer do.
const WEBP_METADATA_FLAGS: u8 = 0x08 | 0x04;

fn big_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

fn big_u32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([*bytes.first()?, *bytes.get(1)?, *bytes.get(2)?, *bytes.get(3)?]))
}

fn little_u32(data: &[u8], at: usize) -> Option<u32> {
    big_u32(data, at).map(u32::swap_bytes)
}

fn tiff_u16(tiff: &[u8], at: usize, big: bool) -> Option<u16> {
    big_u16(tiff, at).map(|value| if big { value } else { value.swap_bytes() })
}

fn tiff_u32(tiff: &[u8], at: usize, big: bool) -> Option<u32> {
    big_u32(tiff, at).map(|value| if big { value } else { value.swap_bytes() })
}

/// Which way up the picture goes, from an `Exif` block. Without it a phone's photo lies on its side.
fn orientation(exif: &[u8]) -> Option<u16> {
    let tiff = exif.strip_prefix(EXIF_HEADER)?;
    let big = match tiff.get(..2)? {
        b"MM" => true,
        b"II" => false,
        _ => return None,
    };
    let table = usize::try_from(tiff_u32(tiff, 4, big)?).ok()?;
    let entries = usize::from(tiff_u16(tiff, table, big)?);
    (0..entries).find_map(|entry| {
        let at = table.checked_add(2)?.checked_add(entry.checked_mul(IFD_ENTRY)?)?;
        (tiff_u16(tiff, at, big)? == ORIENTATION_TAG)
            .then(|| tiff_u16(tiff, at + 8, big))
            .flatten()
            .filter(|value| (UPRIGHT + 1..=LAST_ORIENTATION).contains(value))
    })
}

/// The smallest `Exif` block there is, and it says only which way up.
fn orientation_block(value: u16) -> Vec<u8> {
    let mut tiff = b"MM\0*".to_vec();
    tiff.extend_from_slice(&8_u32.to_be_bytes());
    tiff.extend_from_slice(&1_u16.to_be_bytes());
    tiff.extend_from_slice(&ORIENTATION_TAG.to_be_bytes());
    tiff.extend_from_slice(&SHORT_TYPE.to_be_bytes());
    tiff.extend_from_slice(&1_u32.to_be_bytes());
    tiff.extend_from_slice(&value.to_be_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0_u32.to_be_bytes());
    let length = u16::try_from(2 + EXIF_HEADER.len() + tiff.len()).unwrap_or(u16::MAX);
    let mut block = vec![MARKER, MARKER_APP1];
    block.extend_from_slice(&length.to_be_bytes());
    block.extend_from_slice(EXIF_HEADER);
    block.extend_from_slice(&tiff);
    block
}

fn keeps_jpeg_segment(kind: u8, body: &[u8]) -> bool {
    match kind {
        MARKER_APP0 => body.starts_with(b"JFIF\0"),
        MARKER_APP2 => body.starts_with(b"ICC_PROFILE\0"),
        MARKER_APP14 => body.starts_with(b"Adobe"),
        MARKER_APP1..=MARKER_APP15 | MARKER_COMMENT => false,
        _ => true,
    }
}

/// The offset of the marker that ends compressed data, which holds `FF 00` and restart markers.
fn end_of_scan(data: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    loop {
        at += data.get(at..)?.iter().position(|byte| *byte == MARKER)?;
        let next = *data.get(at + 1)?;
        if next != 0 && next != MARKER && !(0xd0..=0xd7).contains(&next) {
            return Some(at);
        }
        at += 1;
    }
}

fn clean_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = JPEG_START.to_vec();
    let mut after_header = out.len();
    let mut turned = None;
    let mut at = JPEG_START.len();
    loop {
        if *data.get(at)? != MARKER {
            return None;
        }
        let kind = *data.get(at + 1)?;
        if kind == MARKER {
            at += 1;
            continue;
        }
        if kind == MARKER_END {
            out.extend_from_slice(&[MARKER, MARKER_END]);
            break;
        }
        let length = usize::from(big_u16(data, at + 2)?);
        let segment = data.get(at..at + 2 + length)?;
        let body = segment.get(4..)?;
        if kind == MARKER_APP1 && turned.is_none() {
            turned = orientation(body);
        }
        if kind == MARKER_SCAN {
            let end = end_of_scan(data, at + 2 + length)?;
            out.extend_from_slice(data.get(at..end)?);
            at = end;
            continue;
        }
        if keeps_jpeg_segment(kind, body) {
            let first = out.len() == JPEG_START.len();
            out.extend_from_slice(segment);
            if first && kind == MARKER_APP0 {
                after_header = out.len();
            }
        }
        at += 2 + length;
    }
    if let Some(value) = turned {
        out.splice(after_header..after_header, orientation_block(value));
    }
    Some(out)
}

fn clean_png(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = PNG_START.to_vec();
    let mut at = PNG_START.len();
    loop {
        let length = usize::try_from(big_u32(data, at)?).ok()?;
        let kind = data.get(at + 4..at + 8)?;
        let whole = data.get(at..at.checked_add(length)?.checked_add(12)?)?;
        if PNG_KEPT.iter().any(|kept| kept.as_slice() == kind) {
            out.extend_from_slice(whole);
        }
        if kind == PNG_LAST.as_slice() {
            return Some(out);
        }
        at += whole.len();
    }
}

fn clean_webp(data: &[u8]) -> Option<Vec<u8>> {
    let declared = usize::try_from(little_u32(data, 4)?).ok()?.checked_add(8)?;
    let end = declared.min(data.len());
    let mut out = data.get(..WEBP_HEADER)?.to_vec();
    let mut at = WEBP_HEADER;
    while at < end {
        let kind = data.get(at..at + 4)?;
        let length = usize::try_from(little_u32(data, at + 4)?).ok()?;
        let padded = length.checked_add(length % 2)?;
        let whole = data.get(at..at.checked_add(8)?.checked_add(padded)?)?;
        if WEBP_KEPT.iter().any(|kept| kept.as_slice() == kind) {
            let start = out.len();
            out.extend_from_slice(whole);
            if kind == WEBP_EXTENDED.as_slice() {
                *out.get_mut(start + 8)? &= !WEBP_METADATA_FLAGS;
            }
        }
        at += whole.len();
    }
    let size = u32::try_from(out.len().checked_sub(8)?).ok()?;
    out.get_mut(4..8)?.copy_from_slice(&size.to_le_bytes());
    Some(out)
}

/// Goes by the file's first bytes, not its name: a photo called `notes.txt` is still a photo.
#[must_use]
pub fn without_metadata(data: Vec<u8>) -> Vec<u8> {
    let cleaned = if data.starts_with(JPEG_START) {
        clean_jpeg(&data)
    } else if data.starts_with(PNG_START) {
        clean_png(&data)
    } else if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        clean_webp(&data)
    } else {
        None
    };
    cleaned.unwrap_or(data)
}

/// For another module's tests, which need a photo that is turned.
#[cfg(test)]
pub mod tests_support {
    #[must_use]
    pub fn turned_jpeg(plain: &[u8], orientation: u16) -> Vec<u8> {
        let block = super::orientation_block(orientation);
        [plain.get(..2).unwrap_or_default(), &block, plain.get(2..).unwrap_or_default()].concat()
    }
}

#[cfg(test)]
mod tests;
