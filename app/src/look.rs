//! How the app looks, from the preferences. Every window has its own copy of `Theme`, so each one
//! is dressed as it opens.

use crate::{App, Theme, preferences};
use image::{ImageFormat, RgbaImage, imageops, imageops::FilterType};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, winit_030::WinitWindowAccessor};

const MAIL_TEXT: [f32; 3] = [0.9, 1.0, 1.2];
pub const ZOOMS: [u32; 5] = [90, 100, 110, 125, 150];

pub fn mail_text() -> f32 {
    let chosen = usize::from(preferences::get().mail_text);
    MAIL_TEXT.get(chosen).copied().unwrap_or(1.0)
}

const LOGO: &[u8] = include_bytes!("../ui/logo/logo.png");

/// Scales a picture with soft edges. Colour is weighed by its alpha on the way, or the black of fully
/// transparent pixels bleeds in and leaves a dark rim.
pub fn shrink(picture: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let mut weighed = picture.clone();
    for pixel in weighed.pixels_mut() {
        let [red, green, blue, alpha] = pixel.0;
        let weigh = |value: u8| u8::try_from(u32::from(value) * u32::from(alpha) / 255).unwrap_or(u8::MAX);
        pixel.0 = [weigh(red), weigh(green), weigh(blue), alpha];
    }
    let mut scaled = imageops::resize(&weighed, width, height, FilterType::CatmullRom);
    for pixel in scaled.pixels_mut() {
        let [red, green, blue, alpha] = pixel.0;
        if alpha > 0 {
            let restore = |value: u8| (u32::from(value) * 255 / u32::from(alpha)).min(255) as u8;
            pixel.0 = [restore(red), restore(green), restore(blue), alpha];
        }
    }
    scaled
}

/// Shrunk from the large picture to the pixels it will fill, for an even edge.
fn sharp_logo(side: u32) -> Option<Image> {
    let large = image::load_from_memory_with_format(LOGO, ImageFormat::Png).ok()?.to_rgba8();
    let small = shrink(&large, side, side);
    Some(Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(small.as_raw(), side, side)))
}

pub fn dress(theme: &Theme<'_>) {
    let chosen = preferences::get();
    theme.set_choice(i32::from(chosen.theme));
    theme.set_palette(i32::from(chosen.palette));
    theme.set_compact(chosen.compact);
    theme.set_rows(chosen.rows);
    theme.set_mail_text(mail_text());
    theme.set_reveal_private(!chosen.hide_private);
}

/// Sheets, menus and the window's own buttons are the system's, so it has to be told as well.
#[cfg(target_os = "macos")]
fn tell_the_system(choice: u8) {
    use objc2::{MainThreadMarker, msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::{NSAppearance, NSApplication};
    use objc2_foundation::NSString;

    let Some(marker) = MainThreadMarker::new() else { return };
    let name = match choice {
        1 => Some("NSAppearanceNameAqua"),
        2 => Some("NSAppearanceNameDarkAqua"),
        _ => None,
    };
    let appearance: Option<Retained<NSAppearance>> =
        name.and_then(|name| NSAppearance::appearanceNamed(&NSString::from_str(name)));
    let application = NSApplication::sharedApplication(marker);
    let application: &AnyObject = &application;
    // Safety: the application takes an appearance or nothing, on the main thread.
    let _: () = unsafe { msg_send![application, setAppearance: appearance.as_deref()] };
}

/// Windows takes the taskbar's picture apart from the title bar's, and the toolkit passes on only the title bar's when
/// the icon is set before the window exists, as `app.slint` sets it: the taskbar showed Windows' blank program.
#[cfg(target_os = "windows")]
pub fn taskbar_icon(app: &App) {
    use slint::winit_030::{WinitWindowAccessor, winit::platform::windows::WindowExtWindows};
    let Some(icon) = window_icon() else { return };
    app.window().with_winit_window(|window| window.set_taskbar_icon(Some(icon)));
}

/// The app's envelope for a window's title bar. macOS draws none there.
#[cfg(not(target_os = "macos"))]
pub fn window_icon() -> Option<slint::winit_030::winit::window::Icon> {
    let picture = image::load_from_memory(include_bytes!("../icons/128x128.png")).ok()?.to_rgba8();
    let (width, height) = picture.dimensions();
    slint::winit_030::winit::window::Icon::from_rgba(picture.into_raw(), width, height).ok()
}

#[cfg(not(target_os = "windows"))]
pub const fn taskbar_icon(_app: &App) {}

/// The title bar is the system's. Windows draws it light whatever the app is, unless told; with no
/// choice, winit follows the system's own setting.
#[cfg(not(target_os = "macos"))]
pub fn title_bar_theme() -> Option<slint::winit_030::winit::window::Theme> {
    use slint::winit_030::winit::window::Theme;
    match preferences::get().theme {
        1 => Some(Theme::Light),
        2 => Some(Theme::Dark),
        _ => None,
    }
}

#[cfg(not(target_os = "macos"))]
fn tell_the_system(app: &App) {
    use slint::winit_030::WinitWindowAccessor;
    app.window().with_winit_window(|window| window.set_theme(title_bar_theme()));
}

/// Again whenever the scale changes.
pub fn sharpen_logo(app: &App) {
    let theme = app.global::<Theme>();
    let side = crate::numbers::pixels(theme.get_logo_size() * app.window().scale_factor());
    if let Some(picture) = sharp_logo(side) {
        theme.set_logo(picture);
    }
}

pub fn apply(app: &App) {
    dress(&app.global::<Theme>());
    #[cfg(target_os = "macos")]
    tell_the_system(preferences::get().theme);
    #[cfg(not(target_os = "macos"))]
    tell_the_system(app);
}

pub fn language() -> String {
    let chosen = preferences::get().language;
    if !chosen.is_empty() {
        return chosen;
    }
    sys_locale::get_locale()
        .and_then(|locale| locale.split(['-', '_']).next().map(str::to_lowercase))
        .unwrap_or_default()
}

pub fn apply_language() {
    let language = language();
    proton_core::timing::set_language(&language);
    let _ = slint::select_bundled_translation(&language);
}

pub fn zoom() -> f64 {
    f64::from(preferences::get().zoom) / 100.0
}

/// The toolkit lays everything out again for a new scale, which is what zoom means here. The system
/// says the scale again after a move to another display, and this is then called once more.
pub fn apply_zoom(app: &App) {
    let window = app.window();
    let Some(system) = window.with_winit_window(slint::winit_030::winit::window::Window::scale_factor) else { return };
    let scale_factor = crate::numbers::scale(system * zoom());
    if (window.scale_factor() - scale_factor).abs() > f32::EPSILON {
        crate::rescale::rescale(window, scale_factor);
    }
    crate::mail_view::zoom_to(zoom());
    sharpen_logo(app);
}
