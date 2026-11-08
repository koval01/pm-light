//! Where the window stood and how large it was, for the next start. A display may be gone by then,
//! so a place is only used if it is on a display that is there.

use crate::{App, preferences};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, winit_030::WinitWindowAccessor};

/// How much of the top left corner must be on a display for the window to be reachable there.
const REACHABLE: i32 = 40;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: i32, y: i32) -> WindowPlace {
        WindowPlace { x, y, width: 800, height: 600 }
    }

    #[test]
    fn a_place_on_a_display_is_kept() {
        assert!(is_on(&at(100, 100), (0, 0, 1920, 1080)));
        assert!(is_on(&at(2000, 100), (1920, 0, 1920, 1080)));
        assert!(is_on(&at(-500, 100), (-1920, 0, 1920, 1080)));
    }

    #[test]
    fn a_place_on_a_display_that_is_gone_is_not() {
        assert!(!is_on(&at(4000, 100), (0, 0, 1920, 1080)));
        assert!(!is_on(&at(100, 3000), (0, 0, 1920, 1080)));
    }

    #[test]
    fn a_window_needs_some_of_itself_on_the_display() {
        assert!(!is_on(&at(1900, 100), (0, 0, 1920, 1080)));
        assert!(!is_on(&at(100, -10), (0, 0, 1920, 1080)));
    }
}
