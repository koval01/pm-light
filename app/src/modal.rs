//! A window laid over the main one, which stays out of reach until this one closes.
//! macOS has this as a sheet. Windows gets an owned window and a disabled owner.

use crate::App;
use slint::{
    ComponentHandle, LogicalSize, Timer, Window,
    winit_030::{WinitWindowAccessor, winit::window::WindowButtons},
};
use std::time::Duration;
use wry::raw_window_handle::HasWindowHandle;

const MARGIN: f32 = 32.0;
const FRAME: Duration = Duration::from_millis(8);

/// Runs `then` once the system has the window. Until some time after `show` it has no handle, and
/// neither a webview nor a sheet can be attached to it.
pub fn after_show<C: ComponentHandle + 'static>(component: &C, then: impl FnOnce(C) + 'static) {
    let weak = component.as_weak();
    Timer::single_shot(FRAME, move || {
        let Some(component) = weak.upgrade() else { return };
        if component.window().window_handle().window_handle().is_ok() {
            then(component);
        } else {
            after_show(&component, then);
        }
    });
}

#[cfg(target_os = "macos")]
pub use platform::native as native_window;

pub fn present(parent: &Window, overlay: &Window) {
    #[cfg(not(target_os = "macos"))]
    centre(parent, overlay);
    overlay.with_winit_window(|window| {
        window.set_enabled_buttons(WindowButtons::CLOSE);
        #[cfg(not(target_os = "macos"))]
        {
            window.set_theme(crate::look::title_bar_theme());
            // A window of its own shows a blank page in its title bar, unless given the app's.
            window.set_window_icon(crate::look::window_icon());
        }
    });
    platform::present(parent, overlay);
}

/// Over the middle of the window that asked. Left to the system, Windows put it wherever it put the last new window,
/// and a letter as tall as the main window then hung past the bottom of the screen. macOS hangs a sheet from its parent.
#[cfg(not(target_os = "macos"))]
fn centre(parent: &Window, overlay: &Window) {
    use slint::winit_030::winit::dpi::PhysicalPosition;
    let Some(Ok(place)) = parent.with_winit_window(slint::winit_030::winit::window::Window::outer_position) else {
        return;
    };
    let Some(room) = parent.with_winit_window(slint::winit_030::winit::window::Window::outer_size) else { return };
    let Some(size) = overlay.with_winit_window(slint::winit_030::winit::window::Window::outer_size) else { return };
    let along = |start: i32, outer: u32, inner: u32| {
        let spare = i64::from(outer) - i64::from(inner);
        i32::try_from(i64::from(start) + spare.max(0) / 2).unwrap_or(start)
    };
    let at = PhysicalPosition::new(along(place.x, room.width, size.width), along(place.y, room.height, size.height));
    overlay.with_winit_window(|window| window.set_outer_position(at));
}

/// The size wanted, kept within the main window, which a sheet should not hang out of.
pub fn fitted(parent: &App, width: f32, height: f32) -> LogicalSize {
    let scale = parent.window().scale_factor();
    let room = parent.window().size().to_logical(scale);
    LogicalSize::new(width.min(room.width - MARGIN), height.min(room.height - MARGIN))
}

/// Goes before the overlay is hidden: the system still has to let go of it.
#[cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    expect(clippy::missing_const_for_fn, reason = "it does work on macOS and Windows, and Linux has nothing to undo")
)]
pub fn dismiss(parent: &Window, overlay: &Window) {
    platform::dismiss(parent, overlay);
}

#[cfg(target_os = "macos")]
mod platform {
    use super::FRAME;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindow, NSWindowStyleMask};
    use slint::{Timer, Window};
    use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub fn native(window: &Window) -> Option<Retained<NSWindow>> {
        let handle = window.window_handle();
        let handle = handle.window_handle().ok()?;
        let RawWindowHandle::AppKit(view) = handle.as_raw() else { return None };
        // Safety: the view belongs to an open window, on the thread that owns it.
        let view = unsafe { view.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    /// The toolkit puts a window on screen after its first frame, and that would tear a sheet off
    /// its parent. So the window waits unseen until it has been shown, and only then becomes one.
    fn attach_once_shown(parent: Retained<NSWindow>, overlay: Retained<NSWindow>) {
        if overlay.isVisible() {
            overlay.orderOut(None);
            // A title bar would bring the three window buttons. The toolkit draws the one close button.
            overlay.setStyleMask(NSWindowStyleMask::Resizable);
            overlay.setAlphaValue(1.0);
            parent.beginSheet_completionHandler(&overlay, None);
        } else {
            Timer::single_shot(FRAME, move || attach_once_shown(parent, overlay));
        }
    }

    pub fn present(parent: &Window, overlay: &Window) {
        if let (Some(parent), Some(overlay)) = (native(parent), native(overlay)) {
            overlay.setAlphaValue(0.0);
            attach_once_shown(parent, overlay);
        }
    }

    pub fn dismiss(parent: &Window, overlay: &Window) {
        if let (Some(parent), Some(overlay)) = (native(parent), native(overlay)) {
            parent.endSheet(&overlay);
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use slint::Window;
    use windows_sys::Win32::{
        Foundation::HWND,
        UI::{
            Input::KeyboardAndMouse::EnableWindow,
            WindowsAndMessaging::{GWLP_HWNDPARENT, SetForegroundWindow, SetWindowLongPtrW},
        },
    };
    use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    fn native(window: &Window) -> Option<HWND> {
        let handle = window.window_handle();
        let handle = handle.window_handle().ok()?;
        let RawWindowHandle::Win32(window) = handle.as_raw() else { return None };
        Some(window.hwnd.get() as HWND)
    }

    pub fn present(parent: &Window, overlay: &Window) {
        if let (Some(parent), Some(overlay)) = (native(parent), native(overlay)) {
            // Safety: both handles are of open windows on this thread.
            unsafe {
                SetWindowLongPtrW(overlay, GWLP_HWNDPARENT, parent as isize);
                EnableWindow(parent, 0);
            }
        }
    }

    pub fn dismiss(parent: &Window, _overlay: &Window) {
        if let Some(parent) = native(parent) {
            // Safety: as above. The owner comes back first, or Windows raises some other app.
            unsafe {
                EnableWindow(parent, 1);
                SetForegroundWindow(parent);
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use slint::Window;

    pub const fn present(_parent: &Window, _overlay: &Window) {}

    pub const fn dismiss(_parent: &Window, _overlay: &Window) {}
}
