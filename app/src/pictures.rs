//! Senders' logos and attachments' thumbnails made into pixels: decoded, made round or given round corners, and shrunk
//! with the smoothness drawn in. Work for a worker thread; the window's thread only wraps what comes out.

use crate::numbers;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{RgbaImage, imageops};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// A logo as it crosses to the window's thread: images of the toolkit stay on that side.
pub enum Logo {
    Pixels(Vec<u8>, u32, u32),
    Vector(Vec<u8>),
}

/// A picture in a data URL, as Proton sends logos and thumbnails. Anything else, or what does not decode, is nothing.
pub fn logo_of(data_url: &str) -> Option<Logo> {
    let (kind, encoded) = data_url.split_once(";base64,")?;
    let bytes = STANDARD.decode(encoded).ok()?;
    if kind.ends_with("svg+xml") {
        return Some(Logo::Vector(bytes));
    }
    let picture = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = picture.dimensions();
    Some(Logo::Pixels(picture.into_raw(), width, height))
}

/// How much larger a picture is drawn before it shrinks to its pixels. The renderer neither clips to a
/// curve nor smooths what it scales, so the smoothness has to be in the picture.
const OVERSAMPLE: u32 = 4;
/// As a share of its side.
const THUMBNAIL_CORNER: f32 = 0.18;
/// Keeps a white logo apart from a white panel, in either theme.
const RING: [u8; 4] = [0, 0, 0, 31];
const RING_WIDTH: f32 = 1.0;

/// A disc holding the logo, drawn large with hard edges and then shrunk: the edge comes out even
/// all the way round, and the ring is part of it, so the two cannot disagree.
fn disc(source: &RgbaImage, side: u32) -> RgbaImage {
    let large = side * OVERSAMPLE;
    let longest = source.width().max(source.height()).max(1);
    let scaled = |length: u32| (length * large / longest).max(1);
    let fitted = crate::look::shrink(source, scaled(source.width()), scaled(source.height()));
    let mut canvas = RgbaImage::from_pixel(large, large, image::Rgba([255, 255, 255, 255]));
    let (x, y) = ((large - fitted.width()) / 2, (large - fitted.height()) / 2);
    imageops::overlay(&mut canvas, &fitted, i64::from(x), i64::from(y));
    let radius = numbers::float(large) / 2.0;
    let ring = RING_WIDTH.mul_add(-numbers::float(OVERSAMPLE), radius);
    let [ring_red, ring_green, ring_blue, ring_alpha] = RING.map(f32::from);
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        let reach = (numbers::float(x) + 0.5 - radius).hypot(numbers::float(y) + 0.5 - radius);
        if reach > radius {
            *pixel = image::Rgba([255, 255, 255, 0]);
        } else if reach > ring {
            let share = ring_alpha / 255.0;
            let blend = |value: u8, over: f32| numbers::channel(over.mul_add(share, f32::from(value) * (1.0 - share)));
            *pixel = image::Rgba([
                blend(pixel.0[0], ring_red),
                blend(pixel.0[1], ring_green),
                blend(pixel.0[2], ring_blue),
                255,
            ]);
        }
    }
    crate::look::shrink(&canvas, side, side)
}

/// A logo made round and shrunk, as pixels: work for a worker thread. The window's thread only wraps the result,
/// since decoding and shrinking there held it for a third of a second and the splash's envelope jerked.
pub fn picture_pixels(logo: &Logo, side: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let source = match logo {
        Logo::Pixels(pixels, width, height) => RgbaImage::from_raw(*width, *height, pixels.clone())?,
        Logo::Vector(bytes) => {
            let drawn = Image::load_from_svg_data(bytes).ok()?.to_rgba8()?;
            RgbaImage::from_raw(drawn.width(), drawn.height(), drawn.as_bytes().to_vec())?
        }
    };
    let finished = disc(&source, side);
    Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(finished.as_raw(), side, side))
}

/// A corner of a thumbnail, drawn large with a hard edge and shrunk, as a logo's disc is. Pixels, made off the
/// window's thread.
pub fn thumbnail_pixels(logo: &Logo, side: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let Logo::Pixels(pixels, width, height) = logo else { return None };
    let source = RgbaImage::from_raw(*width, *height, pixels.clone())?;
    let large = side * OVERSAMPLE;
    let mut canvas = crate::look::shrink(&source, large, large);
    let corner = numbers::float(large) * THUMBNAIL_CORNER;
    let reach = numbers::float(large) - corner;
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        let (x, y) = (numbers::float(x) + 0.5, numbers::float(y) + 0.5);
        let across = (corner - x).max(x - reach).max(0.0);
        let down = (corner - y).max(y - reach).max(0.0);
        if across.hypot(down) > corner {
            pixel.0[3] = 0;
        }
    }
    let finished = crate::look::shrink(&canvas, side, side);
    Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(finished.as_raw(), side, side))
}

#[cfg(test)]
mod tests;
