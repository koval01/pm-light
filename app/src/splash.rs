//! The envelope on the splash, drawn as whole frames, each from vectors at the exact pixels of the screen it is shown
//! on. The renderer does not smooth what it scales, so a picture stretched on every frame came out stepped, and
//! layers stretched one by one showed each other's edges. Here nothing is stretched: a frame is one drawing, and the
//! window only shows it.
//!
//! The frames are made on a thread of their own at the start, and let go once the envelope has landed.

use crate::App;
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

type Rgba8PixelBuffer = SharedPixelBuffer<Rgba8Pixel>;
use std::{f32::consts::PI, fmt::Write as _, rc::Rc};

/// One turn of the envelope, as `app.slint` times it.
pub const FRAMES: u32 = 78;
/// The sizes the envelope passes through on its way to the logo, one for each frame of the flight.
pub const FLIGHT: u32 = 32;
/// The splash's envelope and the logo it becomes, in points.
pub const LARGE: f32 = 88.0;
pub const SMALL: f32 = 26.0;
/// The drawing's own units: 120 across, as the logo's sources, and room above the envelope for the letter and the
/// lift, below it for the shadow.
const WIDE: f32 = 120.0;
const TOP: f32 = -32.0;
const TALL: f32 = 165.0;

const GRADIENTS: &str = r##"<defs><linearGradient id="body" x1="0" y1="0" x2="0.9" y2="1"><stop offset="0" stop-color="#6fb2ff"/><stop offset="1" stop-color="#2f62dc"/></linearGradient><linearGradient id="flap" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#cfe6ff"/><stop offset="1" stop-color="#7fb4ff"/></linearGradient><linearGradient id="inner" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#1c3f9e"/><stop offset="1" stop-color="#2a55c4"/></linearGradient><filter id="soft" x="-20%" y="-100%" width="140%" height="300%"><feGaussianBlur stdDeviation="4"/></filter></defs>"##;
/// The inside, a hair smaller than the pocket and the flap that cover it, so that its dark edge never shows through
/// theirs where the two would meet.
const BACK: &str = r#"<rect x="12.6" y="40.6" width="94.8" height="54" rx="11.4" fill="url(#inner)"/>"#;
const POCKET: &str =
    r#"<path d="M12 52 L60 82 L108 52 V94 A12 12 0 0 1 96 106 H24 A12 12 0 0 1 12 94 Z" fill="url(#body)"/>"#;
const FLAP: &str = "M24 40 H96 A12 12 0 0 1 108 52 L66 80 A13 13 0 0 1 54 80 L12 52 A12 12 0 0 1 24 40 Z";
const PAPER: &str = r##"<rect x="24" y="30" width="72" height="62" rx="6" fill="#ffffff"/><rect x="34" y="44" width="52" height="5" rx="2.5" fill="#b9cdf2"/><rect x="34" y="56" width="52" height="5" rx="2.5" fill="#cfdcf6"/><rect x="34" y="68" width="34" height="5" rx="2.5" fill="#cfdcf6"/>"##;

/// Where everything is at one moment of the turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// 1 is shut, -1 wide open: the flap turns over on its hinge.
    pub fold: f32,
    /// How far the letter has come down, in units, and how far it is turned, in degrees.
    pub letter_y: f32,
    pub letter_turn: f32,
    pub letter_shown: f32,
    /// The envelope settling as the flap lands: 0 still, 1 most pressed.
    pub squash: f32,
    /// How high it floats, 0 to 1, and its sway, in degrees.
    pub lift: f32,
    pub sway: f32,
}

fn ramp(phase: f32, from: f32, to: f32) -> f32 {
    ((phase - from) / (to - from)).clamp(0.0, 1.0)
}

fn eased(x: f32) -> f32 {
    x * x * 2.0f32.mul_add(-x, 3.0)
}

/// The moment `phase`, from 0 to 1, of the turn: the flap opens, the letter goes in with a slight turn, the flap
/// shuts and the envelope gives a little, then it rises, sways and comes down again. It ends where it began.
#[must_use]
pub fn pose(phase: f32) -> Pose {
    let phase = phase.rem_euclid(1.0);
    let open = eased(ramp(phase, 0.0, 0.12));
    let shut = eased(ramp(phase, 0.34, 0.46));
    let down = eased(ramp(phase, 0.08, 0.34));
    let rise = eased(ramp(phase, 0.52, 0.76)) * (1.0 - eased(ramp(phase, 0.80, 0.98)));
    Pose {
        fold: 2.0f32.mul_add(shut, 2.0f32.mul_add(-open, 1.0)),
        letter_y: 60.0f32.mul_add(down, -52.0),
        letter_turn: -10.0 * (1.0 - down),
        letter_shown: ramp(phase, 0.06, 0.12),
        squash: (ramp(phase, 0.44, 0.56) * PI).sin(),
        lift: rise,
        sway: 3.0 * (ramp(phase, 0.52, 0.98) * 2.0 * PI).sin() * rise,
    }
}

/// One frame, as an SVG drawn to `width` by `height` pixels.
#[must_use]
pub fn frame_svg(pose: &Pose, width: u32, height: u32) -> String {
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 {TOP} {WIDE} {TALL}">{GRADIENTS}"#
    );
    // The shadow stays on the ground as the envelope rises, smaller and fainter.
    let spread = 0.3f32.mul_add(-pose.lift, 1.0);
    let _ = write!(
        svg,
        r##"<ellipse cx="60" cy="120" rx="{}" ry="5" fill="#000" opacity="{}" filter="url(#soft)"/>"##,
        44.0 * spread,
        0.25f32.mul_add(-pose.lift, 0.42)
    );
    let (wide, low) = (0.04f32.mul_add(pose.squash, 1.0), 0.05f32.mul_add(-pose.squash, 1.0));
    let _ = write!(
        svg,
        r#"<g transform="translate(0 {}) rotate({} 60 73) translate(60 106) scale({wide} {low}) translate(-60 -106)">"#,
        -16.0 * pose.lift,
        pose.sway
    );
    svg.push_str(BACK);
    let flap = |fill: &str| {
        format!(
            r#"<path d="{FLAP}" fill="url(#{fill})" transform="translate(0 40) scale(1 {}) translate(0 -40)"/>"#,
            pose.fold
        )
    };
    if pose.fold < 0.0 {
        svg.push_str(&flap("inner"));
    }
    if pose.letter_shown > 0.0 {
        let _ = write!(
            svg,
            r#"<g opacity="{}" transform="translate(0 {}) rotate({} 60 61)">{PAPER}</g>"#,
            pose.letter_shown, pose.letter_y, pose.letter_turn
        );
    }
    svg.push_str(POCKET);
    if pose.fold >= 0.0 {
        svg.push_str(&flap("flap"));
    }
    svg.push_str("</g></svg>");
    svg
}

/// The finished logo at one size, as a square SVG of `side` pixels.
#[must_use]
pub fn logo_svg(side: u32) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{side}" height="{side}" viewBox="0 13 120 120">{GRADIENTS}{BACK}{POCKET}<path d="{FLAP}" fill="url(#flap)"/></svg>"#
    )
}

/// The size, in points, of the `step`th picture of the flight: from the splash's envelope down to the logo.
#[must_use]
pub fn flight_size(step: u32) -> f32 {
    let share = crate::numbers::float(step) / crate::numbers::float(FLIGHT - 1);
    (SMALL - LARGE).mul_add(share.clamp(0.0, 1.0), LARGE)
}

fn drawn(svg: &str) -> Option<Rgba8PixelBuffer> {
    Image::load_from_svg_data(svg.as_bytes()).ok()?.to_rgba8()
}

fn pixels(points: f32, scale: f32) -> u32 {
    crate::numbers::pixels(points * scale).max(1)
}

/// Every frame of the turn and every picture of the flight, at `scale`.
fn made(scale: f32) -> Option<(Vec<Rgba8PixelBuffer>, Vec<Rgba8PixelBuffer>)> {
    let (width, height) = (pixels(LARGE, scale), pixels(LARGE * TALL / WIDE, scale));
    let turn = (0..FRAMES)
        .map(|frame| {
            drawn(&frame_svg(&pose(crate::numbers::float(frame) / crate::numbers::float(FRAMES)), width, height))
        })
        .collect::<Option<Vec<_>>>()?;
    let flight =
        (0..FLIGHT).map(|step| drawn(&logo_svg(pixels(flight_size(step), scale)))).collect::<Option<Vec<_>>>()?;
    Some((turn, flight))
}

/// Starts making the frames for the window's scale; the splash shows the still logo until they come.
pub fn prepare(app: &App) {
    let scale = app.window().scale_factor();
    let weak = app.as_weak();
    let started = std::time::Instant::now();
    let spawned = std::thread::Builder::new().name("splash".to_string()).spawn(move || {
        let Some((turn, flight)) = made(scale) else {
            log::warn!("the splash's frames could not be drawn");
            return;
        };
        log::debug!("the splash's frames drawn in {} ms", started.elapsed().as_millis());
        let _ = weak.upgrade_in_event_loop(move |app| {
            let images = |buffers: Vec<Rgba8PixelBuffer>| {
                ModelRc::from(Rc::new(VecModel::from(buffers.into_iter().map(Image::from_rgba8).collect::<Vec<_>>())))
            };
            app.set_splash_frames(images(turn));
            app.set_flight_frames(images(flight));
        });
    });
    if spawned.is_err() {
        log::warn!("no thread for the splash's frames");
    }
}

/// Once the envelope has landed: the frames are not needed again, and take memory.
pub fn let_go(app: &App) {
    app.set_splash_frames(ModelRc::default());
    app.set_flight_frames(ModelRc::default());
}

#[cfg(test)]
mod tests;
