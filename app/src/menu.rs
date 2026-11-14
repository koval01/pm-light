//! The menu bar. Besides its own items it is what makes copy and paste work on macOS: there the
//! standard shortcuts reach a webview and a text field only through an Edit menu.

use crate::App;
#[cfg(target_os = "macos")]
use crate::{Menus, tray};
#[cfg(target_os = "macos")]
use slint::ComponentHandle;
#[cfg(target_os = "macos")]
use tray_icon::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};

pub const COMPOSE: &str = "compose";
pub const SYNC: &str = "sync";
pub const PALETTE: &str = "palette";
#[cfg(target_os = "macos")]
pub const EDIT: &str = "edit:";

/// What a page does by itself, and how the same thing is asked of a toolkit field.
#[cfg(target_os = "macos")]
const EDITING: [(&str, &str, &str); 6] = [
    ("undo", "undo:", "CmdOrCtrl+Z"),
    ("redo", "redo:", "CmdOrCtrl+Shift+Z"),
    ("cut", "cut:", "CmdOrCtrl+X"),
    ("copy", "copy:", "CmdOrCtrl+C"),
    ("paste", "paste:", "CmdOrCtrl+V"),
    ("select-all", "selectAll:", "CmdOrCtrl+A"),
];

/// Whether the keyboard is with a webview, which edits by itself, or with the toolkit, which has no
/// part in the system's responders and is handed the shortcut instead.
#[cfg(target_os = "macos")]
fn page_has_focus() -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSView};

    let Some(marker) = MainThreadMarker::new() else { return false };
    let Some(window) = NSApplication::sharedApplication(marker).keyWindow() else { return false };
    let Some(responder) = window.firstResponder() else { return false };
    let Some(mut view) = responder.downcast::<NSView>().ok() else { return false };
    loop {
        if view.class().name().to_string_lossy().contains("WebView") {
            return true;
        }
        // Safety: the view is alive, and the parent comes back retained.
        let Some(parent) = (unsafe { view.superview() }) else { return false };
        view = parent;
    }
}

/// The menu takes these keys on any layout, so this is the one way they arrive.
#[cfg(target_os = "macos")]
pub fn edit(window: &slint::Window, action: &str) {
    use objc2::{MainThreadMarker, runtime::Sel};
    use objc2_app_kit::NSApplication;
    use slint::platform::{Key, WindowEvent};
    use std::ffi::CString;

    let Some((_, selector, _)) = EDITING.iter().find(|(id, ..)| *id == action) else { return };
    if page_has_focus() {
        if let (Some(marker), Ok(name)) = (MainThreadMarker::new(), CString::new(*selector)) {
            // Safety: no target means the first responder, and no sender is needed by these actions.
            let _ = unsafe {
                NSApplication::sharedApplication(marker).sendAction_to_from(Sel::register(&name), None, None)
            };
        }
        return;
    }
    let letter = match action {
        "undo" | "redo" => "z",
        "cut" => "x",
        "copy" => "c",
        "paste" => "v",
        _ => "a",
    };
    let mut held = vec![Key::Control, Key::Meta];
    if action == "redo" {
        held.push(Key::Shift);
    }
    for key in &held {
        window.dispatch_event(WindowEvent::KeyPressed { text: (*key).into() });
    }
    window.dispatch_event(WindowEvent::KeyPressed { text: letter.into() });
    window.dispatch_event(WindowEvent::KeyReleased { text: letter.into() });
    for key in &held {
        window.dispatch_event(WindowEvent::KeyReleased { text: (*key).into() });
    }
}

/// On Windows and Linux the window stays clean, and it handles the same shortcuts itself.
#[cfg(not(target_os = "macos"))]
pub const fn install(_app: &App) {}

#[cfg(target_os = "macos")]
pub fn install(app: &App) {
    let words = app.global::<Menus>();
    let text = |word: slint::SharedString| Some(word.to_string());
    let about = AboutMetadata {
        name: Some("Proton Mail Light".to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        authors: Some(vec!["Yaroslav Koval".to_string()]),
        website: Some(proton_core::REPOSITORY.to_string()),
        comments: Some("Unofficial client. Not affiliated with Proton AG.".to_string()),
        ..Default::default()
    };
    let application = Submenu::new("Proton Mail Light", true);
    let file = Submenu::new(words.get_file(), true);
    let edit = Submenu::new(words.get_edit(), true);
    let view = Submenu::new(words.get_view(), true);
    let window = Submenu::new(words.get_window(), true);
    let compose = tray::shortcut_item(COMPOSE, &words.get_compose(), "CmdOrCtrl+N");
    let sync = tray::shortcut_item(SYNC, &words.get_sync(), "CmdOrCtrl+R");
    let palette = tray::shortcut_item(PALETTE, &words.get_palette(), "CmdOrCtrl+K");

    let built = application
        .append_items(&[
            &PredefinedMenuItem::about(text(words.get_about()).as_deref(), Some(about)),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(text(words.get_services()).as_deref()),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(text(words.get_hide()).as_deref()),
            &PredefinedMenuItem::hide_others(text(words.get_hide_others()).as_deref()),
            &PredefinedMenuItem::show_all(text(words.get_show_all()).as_deref()),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(text(words.get_quit()).as_deref()),
        ])
        .and_then(|()| {
            file.append_items(&[
                &compose,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::close_window(text(words.get_close_window()).as_deref()),
            ])
        })
        .and_then(|()| {
            let items = EDITING.map(|(id, _, keys)| {
                let text = match id {
                    "undo" => words.get_undo(),
                    "redo" => words.get_redo(),
                    "cut" => words.get_cut(),
                    "copy" => words.get_copy(),
                    "paste" => words.get_paste(),
                    _ => words.get_select_all(),
                };
                tray::shortcut_item(&format!("{EDIT}{id}"), &text, keys)
            });
            let [undo, redo, cut, copy, paste, select_all] = &items;
            edit.append_items(&[undo, redo, &PredefinedMenuItem::separator(), cut, copy, paste, select_all])
        })
        .and_then(|()| {
            view.append_items(&[
                &sync,
                &palette,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::fullscreen(text(words.get_fullscreen()).as_deref()),
            ])
        })
        .and_then(|()| {
            window.append_items(&[
                &PredefinedMenuItem::minimize(text(words.get_minimize()).as_deref()),
                &PredefinedMenuItem::maximize(text(words.get_zoom()).as_deref()),
            ])
        });
    let menu = Menu::new();
    match built.and_then(|()| menu.append_items(&[&application, &file, &edit, &view, &window])) {
        Ok(()) => {
            menu.init_for_nsapp();
            window.set_as_windows_menu_for_nsapp();
            tray::keep_menu(menu);
        }
        Err(_) => log::error!("no menu bar"),
    }
}
