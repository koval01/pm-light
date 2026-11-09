//! A new scale for a window, as zoom sets it. The toolkit is told the scale and, with it, the size in points that the
//! window's pixels now make. A scale alone leaves the old size in points, which then needs more pixels than the window
//! has, and the software renderer stops the program on it. Depends on Slint only, so that `tests/zoom.rs` checks this
//! very file.

use slint::platform::WindowEvent;

pub fn rescale(window: &slint::Window, scale_factor: f32) {
    window.dispatch_event(WindowEvent::ScaleFactorChanged { scale_factor });
    window.dispatch_event(WindowEvent::Resized { size: window.size().to_logical(scale_factor) });
}
