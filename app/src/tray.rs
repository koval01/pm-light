//! System menus: the tray, and the menu behind the three dots. Both belong to the system, so they
//! are the same on screen as any other program's, and nothing in the window can sit above them.

use crate::{App, actions, mail_picture, menu, palette, print_dialog, view};
use image::ImageFormat;
use proton_core::timing::SnoozeChoice;
use slint::{ComponentHandle, Weak};
use std::cell::RefCell;
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
// Linux has no way yet to show a system menu over a winit window: see docs/UNCHECKED.md.
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tray_icon::menu::ContextMenu;
use wry::raw_window_handle::HasWindowHandle;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use wry::raw_window_handle::RawWindowHandle;

/// Single-colour on macOS, so that the system can tint it for a light or dark menu bar.
const ICON: &[u8] =
    if cfg!(target_os = "macos") { include_bytes!("../icons/tray.png") } else { include_bytes!("../icons/32x32.png") };
const OPEN: &str = "open";
const QUIT: &str = "quit";
const NAME: &str = "Proton Mail Light";
/// Enough for a menu bar. Anything longer means "a lot".
const MAX_SHOWN: u64 = 999;
const DETAILS: &str = "details";
const PROTECTION: &str = "protection";
const HEADERS: &str = "headers";
const PRINT: &str = "print";
const BLOCK: &str = "block";
const MOVE: &str = "move:";
const LABEL: &str = "label:";
const FROM: &str = "from:";
const SOURCE: &str = "source";
const SAVE_EML: &str = "save-eml";
const COPY_PICTURE: &str = "copy-picture";
const SHARE: &str = "share";
const CHOICE: &str = "choice:";
const LATER: &str = "snooze-later";
const TOMORROW: &str = "snooze-tomorrow";
const NEXT_WEEK: &str = "snooze-next-week";

type Chosen = Box<dyn FnOnce(&App, usize)>;

thread_local! {
    static CHOOSING: RefCell<Option<Chosen>> = const { RefCell::new(None) };
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    static MORE: RefCell<Option<Menu>> = const { RefCell::new(None) };
    #[cfg(target_os = "macos")]
    static BAR: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

fn icon() -> Option<Icon> {
    let picture = image::load_from_memory_with_format(ICON, ImageFormat::Png).ok()?.to_rgba8();
    let (width, height) = picture.dimensions();
    Icon::from_rgba(picture.into_raw(), width, height).ok()
}

fn item(id: &'static str, text: &str) -> MenuItem {
    MenuItem::with_id(id, text, true, None)
}

fn handle(app: &App, id: &str) {
    match id {
        OPEN => drop(app.show()),
        menu::COMPOSE => app.invoke_compose("new".into()),
        menu::SYNC => app.invoke_reload(),
        menu::PALETTE => {
            if let Some(service) = actions::service() {
                palette::open(&service, app);
            }
        }
        QUIT => drop(slint::quit_event_loop()),
        DETAILS => view::open_info(1),
        PROTECTION => view::open_info(2),
        HEADERS => view::open_info(3),
        SOURCE => view::open_info(4),
        SAVE_EML => actions::save_eml(),
        PRINT => {
            if let Some(service) = actions::service() {
                print_dialog::open(&service, app);
            }
        }
        BLOCK => actions::block_sender(),
        COPY_PICTURE | SHARE => {
            if let Some(service) = actions::service() {
                let purpose = if id == SHARE { mail_picture::Purpose::Share } else { mail_picture::Purpose::Copy };
                mail_picture::take(&service, app, purpose);
            }
        }
        LATER => actions::snooze(SnoozeChoice::Later),
        TOMORROW => actions::snooze(SnoozeChoice::Tomorrow),
        NEXT_WEEK => actions::snooze(SnoozeChoice::NextWeek),
        _ => {
            #[cfg(target_os = "macos")]
            if let Some(action) = id.strip_prefix(menu::EDIT) {
                view::with_front_window(app, |window| menu::edit(window, action));
                return;
            }
            if let Some(position) = id.strip_prefix(CHOICE).and_then(|position| position.parse().ok()) {
                if let Some(chosen) = CHOOSING.take() {
                    chosen(app, position);
                }
            } else if let Some(email) = id.strip_prefix(FROM) {
                actions::choose_sender(email);
            } else if let Some(target) = id.strip_prefix(MOVE) {
                actions::move_to(target.to_string());
            } else if let Some(label) = id.strip_prefix(LABEL) {
                let on = !view::current_labels().iter().any(|known| known == label);
                actions::set_label(label.to_string(), on);
            }
        }
    }
}

/// The menu bar lives as long as the app, and a dropped menu is gone from the screen too.
#[cfg(target_os = "macos")]
pub fn keep_menu(menu: Menu) {
    BAR.set(Some(menu));
}

#[cfg(target_os = "macos")]
pub fn shortcut_item(id: &str, text: &str, keys: &str) -> MenuItem {
    MenuItem::with_id(id, text, true, keys.parse().ok())
}

/// Menu events arrive on their own terms, so they are handed to the window's thread.
pub fn listen(app: &App) {
    let weak: Weak<App> = app.as_weak();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let id = event.id().0.clone();
        let _ = weak.upgrade_in_event_loop(move |app| handle(&app, &id));
    }));
}

/// Whether the icon is shown at all: on macOS only when asked for, elsewhere always, since a window closed there
/// has no other way back.
pub fn wanted() -> bool {
    !cfg!(target_os = "macos") || crate::preferences::get().menu_bar_icon
}

/// The icon taken away, as the settings ask.
pub fn remove() {
    TRAY.set(None);
}

/// After the event loop has started: macOS will not take a tray icon sooner.
pub fn install(app: &App) {
    if !wanted() {
        remove();
        return;
    }
    let menu = Menu::new();
    let open = item(OPEN, &app.get_word_open());
    let compose = item(menu::COMPOSE, &app.get_word_compose());
    let sync = item(menu::SYNC, &app.get_word_sync());
    let quit = item(QUIT, &app.get_word_quit());
    if menu.append_items(&[&open, &compose, &sync, &PredefinedMenuItem::separator(), &quit]).is_err() {
        return;
    }
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_icon_as_template(cfg!(target_os = "macos"))
        .with_tooltip(NAME);
    if let Some(icon) = icon() {
        builder = builder.with_icon(icon);
    }
    match builder.build() {
        Ok(tray) => TRAY.set(Some(tray)),
        Err(_) => log::error!("no tray icon"),
    }
    let weak = app.as_weak();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            let _ = weak.upgrade_in_event_loop(|app| drop(app.show()));
        }
    }));
}

fn count_text(unread: u64) -> Option<String> {
    match unread {
        0 => None,
        1..=MAX_SHOWN => Some(unread.to_string()),
        _ => Some(format!("{MAX_SHOWN}+")),
    }
}

/// The title shows on macOS and Linux only, and the tooltip is all Windows has.
pub fn show_unread(_app: &App, count: u64) {
    log::debug!("unread shown: {count}");
    let text = count_text(count);
    TRAY.with_borrow(|tray| {
        if let Some(tray) = tray {
            let tooltip = text.as_ref().map_or_else(|| NAME.to_string(), |text| format!("{NAME} ({text})"));
            tray.set_title(text.as_deref());
            let _ = tray.set_tooltip(Some(tooltip));
        }
    });
    dock_badge(text.as_deref());
}

#[cfg(target_os = "macos")]
fn dock_badge(text: Option<&str>) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let Some(marker) = MainThreadMarker::new() else {
        log::warn!("the Dock badge was asked for off the main thread");
        return;
    };
    let label = text.map(NSString::from_str);
    NSApplication::sharedApplication(marker).dockTile().setBadgeLabel(label.as_deref());
}

#[cfg(not(target_os = "macos"))]
const fn dock_badge(_text: Option<&str>) {}

/// A list to pick from, as the system's own menu, at the pointer.
pub fn choose(app: &App, options: &[String], chosen: impl FnOnce(&App, usize) + 'static) {
    let menu = Menu::new();
    for (position, option) in options.iter().enumerate() {
        let _ = menu.append(&MenuItem::with_id(format!("{CHOICE}{position}"), option, true, None));
    }
    CHOOSING.set(Some(Box::new(chosen)));
    show(app, menu);
}

/// The same, with `marked` ticked: the one in use.
pub fn choose_marked(app: &App, options: &[String], marked: Option<usize>, chosen: impl FnOnce(&App, usize) + 'static) {
    let menu = Menu::new();
    for (position, option) in options.iter().enumerate() {
        let id = format!("{CHOICE}{position}");
        let _ = if Some(position) == marked {
            menu.append(&CheckMenuItem::with_id(id, option, true, true, None))
        } else {
            menu.append(&MenuItem::with_id(id, option, true, None))
        };
    }
    CHOOSING.set(Some(Box::new(chosen)));
    show(app, menu);
}

pub fn show_more(app: &App) {
    let menu = Menu::new();
    let details = item(DETAILS, &app.get_word_details());
    let protection = item(PROTECTION, &app.get_word_protection());
    let headers = item(HEADERS, &app.get_word_headers());
    let source = item(SOURCE, &app.get_word_source());
    let save = item(SAVE_EML, &app.get_word_save_eml());
    let print = item(PRINT, &app.get_word_print());
    let picture = item(COPY_PICTURE, &app.get_word_copy_picture());
    let block = item(BLOCK, &app.get_word_block_sender());
    let line = PredefinedMenuItem::separator();
    if menu.append_items(&[&details, &protection, &headers, &source, &line, &save, &print, &picture]).is_err() {
        return;
    }
    if crate::mail_picture::can_share() && menu.append(&item(SHARE, &app.get_word_share())).is_err() {
        return;
    }
    if menu.append(&block).is_err() {
        return;
    }
    show(app, menu);
}

pub fn show_action_menu(app: &App, kind: &str) {
    let menu = Menu::new();
    match kind {
        "snooze" => {
            for (id, text) in [
                (LATER, app.get_word_later()),
                (TOMORROW, app.get_word_tomorrow()),
                (NEXT_WEEK, app.get_word_next_week()),
            ] {
                let _ = menu.append(&item(id, &text));
            }
        }
        "move" => {
            for (id, name) in view::move_targets() {
                let name = if id == "inbox" { app.get_word_inbox().to_string() } else { name };
                let _ = menu.append(&MenuItem::with_id(format!("{MOVE}{id}"), name, true, None));
            }
        }
        _ => {
            let applied = view::current_labels();
            for (id, name) in view::labels() {
                let on = applied.contains(&id);
                let _ = menu.append(&CheckMenuItem::with_id(format!("{LABEL}{id}"), name, true, on, None));
            }
        }
    }
    show(app, menu);
}

pub fn show_from_menu(window: &slint::Window, addresses: &[String]) {
    let menu = Menu::new();
    for email in addresses {
        let _ = menu.append(&MenuItem::with_id(format!("{FROM}{email}"), email, true, None));
    }
    show_in(window, menu);
}

fn show(app: &App, menu: Menu) {
    show_in(app.window(), menu);
}

fn show_in(window: &slint::Window, menu: Menu) {
    let window = window.window_handle();
    let Ok(handle) = window.window_handle() else { return };
    match handle.as_raw() {
        // Safety: the view belongs to the open window, which outlives this call on its own thread.
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(view) => unsafe {
            menu.show_context_menu_for_nsview(view.ns_view.as_ptr(), None);
        },
        // Safety: as above, for the window's handle.
        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(window) => unsafe {
            menu.show_context_menu_for_hwnd(window.hwnd.get(), None);
        },
        _ => {}
    }
    MORE.set(Some(menu));
}

#[cfg(test)]
mod tests;
