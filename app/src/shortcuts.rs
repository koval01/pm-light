//! Copy, paste and the like on any keyboard layout. The toolkit knows these shortcuts by their
//! letter, so on a Cyrillic layout, where the V key gives a Cyrillic letter, none of them would work. Here such a
//! key is told by its place on the keyboard. A Latin layout passes through as it is, Dvorak included.
//! macOS has this from its menu bar, which `menu.rs` holds.

use crate::{autolock, guard, sync};
use slint::{
    platform::WindowEvent as ToolkitEvent,
    winit_030::{
        CustomApplicationHandler, EventResult,
        winit::{
            event::{ElementState, WindowEvent},
            event_loop::ActiveEventLoop,
            keyboard::{KeyCode, PhysicalKey},
            window::{Window, WindowId},
        },
    },
};

#[derive(Default)]
pub struct AnyLayout {
    command: bool,
}

const fn letter_of(key: KeyCode) -> Option<&'static str> {
    match key {
        KeyCode::KeyA => Some("a"),
        KeyCode::KeyC => Some("c"),
        KeyCode::KeyK => Some("k"),
        KeyCode::KeyV => Some("v"),
        KeyCode::KeyX => Some("x"),
        KeyCode::KeyY => Some("y"),
        KeyCode::KeyZ => Some("z"),
        _ => None,
    }
}

/// The letter a shortcut means, when the layout gave something the toolkit will not recognise.
fn translated(text: Option<&str>, key: KeyCode) -> Option<&'static str> {
    if text.is_some_and(str::is_ascii) { None } else { letter_of(key) }
}

impl CustomApplicationHandler for AnyLayout {
    /// However the app is left, the menu's Quit included, which never comes back to `main`.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) -> EventResult {
        if let Some(app) = crate::actions::main_window() {
            crate::window_place::remember(&app);
        }
        guard::leave_no_trace();
        EventResult::Propagate
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _winit_window: Option<&Window>,
        slint_window: Option<&slint::Window>,
        event: &WindowEvent,
    ) -> EventResult {
        match event {
            WindowEvent::ScaleFactorChanged { .. } => {
                // The system's word on the scale leaves out the zoom.
                slint::Timer::single_shot(std::time::Duration::ZERO, || {
                    if let Some(app) = crate::actions::main_window() {
                        crate::look::apply_zoom(&app);
                    }
                });
                EventResult::Propagate
            }
            WindowEvent::Focused(focused) => {
                autolock::window_focused(*focused);
                if *focused {
                    sync::request_check();
                }
                EventResult::Propagate
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.command = state.control_key();
                EventResult::Propagate
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. }
                if self.command && event.state == ElementState::Pressed =>
            {
                let PhysicalKey::Code(key) = event.physical_key else { return EventResult::Propagate };
                let (Some(letter), Some(window)) = (translated(event.logical_key.to_text(), key), slint_window) else {
                    return EventResult::Propagate;
                };
                window.dispatch_event(ToolkitEvent::KeyPressed { text: letter.into() });
                window.dispatch_event(ToolkitEvent::KeyReleased { text: letter.into() });
                EventResult::PreventDefault
            }
            _ => EventResult::Propagate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cyrillic_key_means_the_letter_in_its_place() {
        assert_eq!(translated(Some("\u{43c}"), KeyCode::KeyV), Some("v"));
        assert_eq!(translated(Some("\u{441}"), KeyCode::KeyC), Some("c"));
        assert_eq!(translated(Some("\u{44f}"), KeyCode::KeyZ), Some("z"));
    }

    #[test]
    fn a_latin_layout_is_left_alone() {
        assert_eq!(translated(Some("v"), KeyCode::KeyV), None);
        assert_eq!(translated(Some("k"), KeyCode::KeyV), None);
    }

    #[test]
    fn other_keys_are_left_alone() {
        assert_eq!(translated(Some("\u{439}"), KeyCode::KeyQ), None);
        assert_eq!(translated(Some("1"), KeyCode::Digit1), None);
    }
}
