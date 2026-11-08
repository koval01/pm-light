//! Where the window stood and how large it was, for the next start. A display may be gone by then,
//! so a place is only used if it is on a display that is there.

use crate::{App, preferences};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, winit_030::WinitWindowAccessor};

/// How much of the top left corner must be on a display for the window to be reachable there.
const REACHABLE: i32 = 40;
/// What the system's own bars take from a display, in points: the taskbar, or the menu bar and the Dock.
const SYSTEM_BARS: f32 = 80.0;
/// The window's least content size, as `app.slint` sets it, in points.
const LEAST: (f32, f32) = (900.0, 580.0);

/// A rectangle in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct WindowPlace {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

pub fn remember(app: &App) {
    let (position, size) = (app.window().position(), app.window().size());
    if size.width == 0 || size.height == 0 {
        return;
    }
    let place = WindowPlace { x: position.x, y: position.y, width: size.width, height: size.height };
    preferences::change(|preferences| preferences.window = Some(place));
}

pub fn restore_size(app: &App) -> bool {
    let Some(place) = preferences::get().window else { return false };
    app.window().set_size(PhysicalSize::new(place.width, place.height));
    true
}

fn is_on(place: &WindowPlace, display: (i32, i32, u32, u32)) -> bool {
    let (x, y, width, height) = display;
    let right = x.saturating_add(i32::try_from(width).unwrap_or(i32::MAX));
    let bottom = y.saturating_add(i32::try_from(height).unwrap_or(i32::MAX));
    place.x >= x - REACHABLE && place.x + REACHABLE <= right && place.y >= y && place.y + REACHABLE <= bottom
}

/// Once the window exists: only then can the displays be asked about.
pub fn restore_position(app: &App) {
    let Some(place) = preferences::get().window else { return };
    let displays = app.window().with_winit_window(|window| {
        let frames = window.available_monitors().map(|display| {
            let (origin, size) = (display.position(), display.size());
            (origin.x, origin.y, size.width, size.height)
        });
        frames.collect::<Vec<_>>()
    });
    if displays.is_some_and(|displays| displays.iter().any(|display| is_on(&place, *display))) {
        app.window().set_position(PhysicalPosition::new(place.x, place.y));
    }
}

/// Where the window's frame must go, and how large its content must be, to lie wholly on `display` above the
/// system's bars; nothing when it already does. `outer` is the frame, `inner` the content within it, `least` the
/// content's minimum: on a display smaller than that, the minimum stays and the window starts at the top left.
#[must_use]
pub fn fitted(display: Frame, outer: Frame, inner: (u32, u32), least: (u32, u32), bars: u32) -> Option<Frame> {
    let room = (display.width, display.height.saturating_sub(bars));
    let right = i64::from(display.x) + i64::from(room.0);
    let bottom = i64::from(display.y) + i64::from(room.1);
    let inside = outer.x >= display.x
        && outer.y >= display.y
        && i64::from(outer.x) + i64::from(outer.width) <= right
        && i64::from(outer.y) + i64::from(outer.height) <= bottom;
    if inside {
        return None;
    }
    let border = (outer.width.saturating_sub(inner.0), outer.height.saturating_sub(inner.1));
    let width = inner.0.min(room.0.saturating_sub(border.0)).max(least.0);
    let height = inner.1.min(room.1.saturating_sub(border.1)).max(least.1);
    let centred = |origin: i32, space: u32, size: u32| {
        origin.saturating_add(i32::try_from(space.saturating_sub(size) / 2).unwrap_or(0))
    };
    Some(Frame {
        x: centred(display.x, room.0, width.saturating_add(border.0)),
        y: centred(display.y, room.1, height.saturating_add(border.1)),
        width,
        height,
    })
}

fn pixels(points: f32, scale: f32) -> u32 {
    crate::numbers::pixels(points * scale)
}

/// Once the window exists and has its place: moved and made smaller where it would not be wholly on its display, as
/// the default size would leave it on a screen smaller than that.
pub fn fit_on_display(app: &App) {
    let scale = app.window().scale_factor();
    let fit = app.window().with_winit_window(|window| {
        let display = window.current_monitor().or_else(|| window.primary_monitor())?;
        let (origin, size) = (display.position(), display.size());
        let place = window.outer_position().ok()?;
        let (outer, inner) = (window.outer_size(), window.inner_size());
        fitted(
            Frame { x: origin.x, y: origin.y, width: size.width, height: size.height },
            Frame { x: place.x, y: place.y, width: outer.width, height: outer.height },
            (inner.width, inner.height),
            (pixels(LEAST.0, scale), pixels(LEAST.1, scale)),
            pixels(SYSTEM_BARS, scale),
        )
    });
    if let Some(Some(frame)) = fit {
        log::info!("the window fitted to its display: {} by {}", frame.width, frame.height);
        app.window().set_size(PhysicalSize::new(frame.width, frame.height));
        app.window().set_position(PhysicalPosition::new(frame.x, frame.y));
    }
}

#[cfg(test)]
mod tests;
