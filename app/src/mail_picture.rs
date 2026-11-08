//! A picture of the open mail, whole: its sender, subject and date over its body, laid out as for printing and drawn
//! by a webview out of sight. It goes to the clipboard, to paste into a chat, or to the system's menu for sharing. A
//! picture leaves Proton's encryption, and the person is told so once, before the first.
//!
//! Pictures from the sender's server are in it only when they are shown in the reader.

use crate::webviews::PageScriptsOff as _;
use crate::{App, actions, preferences, print_dialog, print_view, settings::say, toasts};
use proton_core::{
    PrintScope, Service,
    print::{PrintDocument, PrintOptions},
};
use slint::{ComponentHandle, Timer};
use std::{cell::RefCell, path::PathBuf, sync::Arc, time::Duration};
use wry::{
    PageLoadEvent, Rect, WebView,
    dpi::{LogicalPosition, LogicalSize},
};

pub mod check;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as system;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as system;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as system;

/// About the width of a phone held sideways: a chat shows the picture without shrinking its text much.
pub const WIDTH: f64 = 720.0;
/// Taller than this, in points, the picture is cut: no chat shows it readably, and its pixels would run to hundreds of
/// megabytes.
pub const TALLEST: f64 = 16_000.0;
/// Pixels to a point, for sharp text on any screen it is looked at. WebKitGTK draws at the screen's own.
#[cfg(any(not(target_os = "linux"), test))]
pub const SCALE: f64 = 2.0;
/// Pixels a picture may have, about 100 MB while it is made: a tall mail is drawn less fine than `SCALE`.
#[cfg(any(not(target_os = "linux"), test))]
const MOST_PIXELS: f64 = 25_000_000.0;
/// For the pictures in the mail to be decoded once the page has its size.
const SETTLES: Duration = Duration::from_millis(250);
const MEASURE: &str = "Math.ceil(document.documentElement.scrollHeight)";
/// Paper's margins, where the holes and folds go, are not wanted round a picture: a chat adds its own.
const PICTURE_STYLE: &str = ".print-part{padding:20px 24px}";
/// The name a shared picture has. Nothing of the mail is in it: the file may be seen by the app it goes to.
const SHARED_NAME: &str = "mail.png";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Copy,
    Share,
    /// The made-up mail of `check`, for CI.
    Check,
    /// A real mail of the account check: only its size is kept.
    Account,
}

/// A drawn mail, as PNG.
pub struct Picture {
    pub png: Vec<u8>,
    /// The mail went on past `TALLEST`.
    pub cut: bool,
}

thread_local! {
    static VIEW: RefCell<Option<WebView>> = const { RefCell::new(None) };
    /// How far the picture has come, for the journal and for a check that stops on the way.
    static STAGE: std::cell::Cell<&'static str> = const { std::cell::Cell::new("not begun") };
}

/// The stage the picture has reached, to the journal.
fn reached(stage: &'static str) {
    STAGE.set(stage);
    log::debug!("the mail picture: {stage}");
}

/// The last stage the picture reached.
#[must_use]
pub fn stage() -> &'static str {
    STAGE.get()
}

/// Whether this system can hand a picture to its menu for sharing.
#[must_use]
pub const fn can_share() -> bool {
    system::CAN_SHARE
}

/// The height the page is drawn at, from what it measured, and whether that cuts it. Nonsense is one line.
#[must_use]
pub fn drawn_height(measured: &str) -> (f64, bool) {
    let height = measured.trim().trim_matches('"').parse::<f64>().ok().filter(|height| height.is_finite() && *height >= 1.0);
    height.map_or((1.0, false), |height| (height.min(TALLEST), height > TALLEST))
}

/// Pixels to a point for a page of this size: `SCALE`, or less for a tall one, never under one.
#[cfg(any(not(target_os = "linux"), test))]
#[must_use]
pub fn scale_for(width: f64, height: f64) -> f64 {
    let area = width * height;
    if !area.is_finite() || area <= 0.0 {
        return SCALE;
    }
    (MOST_PIXELS / area).sqrt().clamp(1.0, SCALE)
}

/// What printing lays out, without anything that is only for paper.
#[must_use]
pub fn options(images: bool) -> PrintOptions {
    PrintOptions {
        headers: true,
        images,
        technical: false,
        history: false,
        new_sheet_each: false,
        check_code: false,
        seal: false,
        hole_mark: false,
        ..PrintOptions::default()
    }
}

/// A real mail drawn for the account check, which keeps nothing of it but its size.
pub fn draw_for_check(app: &App, prepared: PrintDocument) {
    draw(app, for_a_picture(prepared), Purpose::Account);
}

/// The picture in an answer of `DevTools`, `{"data": "<base64 PNG>"}`. Nothing for anything else.
#[cfg(any(target_os = "windows", test))]
#[must_use]
pub fn png_of_devtools(answer: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;

    let answer: serde_json::Value = serde_json::from_str(answer).ok()?;
    let png = base64::engine::general_purpose::STANDARD.decode(answer.get("data")?.as_str()?).ok()?;
    png.starts_with(b"\x89PNG").then_some(png)
}

/// A device-independent bitmap, as Windows' clipboard takes one: a 40-byte header, then rows of blue, green, red and
/// a spare byte, from the bottom row up.
#[cfg(any(target_os = "windows", test))]
#[must_use]
pub fn dib(picture: &image::RgbaImage) -> Vec<u8> {
    let (width, height) = picture.dimensions();
    let mut out = Vec::with_capacity(40 + picture.as_raw().len());
    out.extend_from_slice(&40_u32.to_le_bytes());
    out.extend_from_slice(&i32::try_from(width).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&i32::try_from(height).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&32_u16.to_le_bytes());
    // `BI_RGB`, then the size, which may be 0 for it, the resolution and the palette, all unused.
    out.extend_from_slice(&[0; 24]);
    for row in picture.rows().rev() {
        for pixel in row {
            let [red, green, blue, _] = pixel.0;
            out.extend_from_slice(&[blue, green, red, 0]);
        }
    }
    out
}

/// How many rows of a full document drawn `pixels_wide` across keep it within `points_high` points of a page
/// `points_wide` across: WebKitGTK draws the whole document, past where the picture is cut.
#[cfg(any(target_os = "linux", test))]
#[must_use]
pub fn kept_rows(rows: i32, pixels_wide: i32, points_wide: f64, points_high: f64) -> i32 {
    if pixels_wide <= 0 || points_wide.is_nan() || points_wide <= 0.0 || !points_high.is_finite() {
        return rows;
    }
    let wanted = (points_high * f64::from(pixels_wide) / points_wide).ceil();
    if wanted >= f64::from(rows) { rows } else { format!("{:.0}", wanted.max(1.0)).parse().unwrap_or(rows) }
}

/// Cairo's `ARGB32`, premultiplied, in the machine's order (blue first on every system the app runs on), as plain
/// RGBA. Nothing when the sizes do not fit the data.
#[cfg(any(target_os = "linux", test))]
#[must_use]
pub fn rgba_of_argb32(data: &[u8], width: i32, height: i32, stride: i32) -> Option<image::RgbaImage> {
    let (width, height, stride) = (u32::try_from(width).ok()?, u32::try_from(height).ok()?, usize::try_from(stride).ok()?);
    let row_bytes = usize::try_from(width).ok()?.checked_mul(4)?;
    if width == 0 || height == 0 || stride < row_bytes {
        return None;
    }
    let mut picture = image::RgbaImage::new(width, height);
    for (y, row) in picture.rows_mut().enumerate() {
        let start = y.checked_mul(stride)?;
        let source = data.get(start..start.checked_add(row_bytes)?)?;
        let (quads, _) = source.as_chunks::<4>();
        for (pixel, [blue, green, red, alpha]) in row.zip(quads) {
            let straight = |channel: u8| {
                if *alpha == 0 { 0 } else { u8::try_from((u16::from(channel) * 255 + u16::from(*alpha) / 2) / u16::from(*alpha)).unwrap_or(255) }
            };
            pixel.0 = [straight(*red), straight(*green), straight(*blue), *alpha];
        }
    }
    Some(picture)
}

fn folder() -> Option<PathBuf> {
    // Inside the cache folder that quitting empties.
    Some(dirs::cache_dir()?.join(crate::guard::IDENTIFIER).join("shared"))
}

/// Where the page is drawn. An engine that draws a hidden page gets it at the window's corner, hidden; one that does
/// not gets it shown, past the window's left edge, where the window clips it away.
fn out_of_sight() -> LogicalPosition<f64> {
    if system::DRAWS_HIDDEN { LogicalPosition::new(0.0, 0.0) } else { LogicalPosition::new(-(WIDTH + 64.0), 0.0) }
}

fn go_away() {
    VIEW.with_borrow_mut(|view| drop(view.take()));
}

/// Told once: the picture is the mail, readable by whoever gets it.
fn warned(app: &App) -> bool {
    if preferences::get().picture_warned {
        return true;
    }
    let agreed = actions::confirmed(
        &say(app, "picture.warnTitle"),
        &say(app, "picture.warnText"),
        &say(app, "picture.continue"),
        &say(app, "common.cancel"),
    );
    if agreed {
        preferences::change(|preferences| preferences.picture_warned = true);
    }
    agreed
}

/// The open mail as a picture, for `purpose`.
pub fn take(service: &Arc<Service>, app: &App, purpose: Purpose) {
    if purpose == Purpose::Share && !can_share() {
        return;
    }
    if !warned(app) {
        return;
    }
    let (labels, options) = (print_dialog::labels(app), options(!app.get_images_held()));
    let (service, weak) = (service.clone(), app.as_weak());
    log::info!("a picture of a mail asked for, to {}", if purpose == Purpose::Share { "share" } else { "copy" });
    tokio::spawn(async move {
        let prepared = service.print_document(PrintScope::Message, &options, &labels).await;
        let _ = weak.upgrade_in_event_loop(move |app| match prepared {
            Ok(prepared) if !prepared.parts.is_empty() => draw(&app, for_a_picture(prepared), purpose),
            Ok(_) => {}
            Err(error) => {
                log::warn!("no picture of a mail: {}", error.kind());
                actions::toast(toasts::FAILED);
            }
        });
    });
}

/// The printed layout with a picture's margins.
#[must_use]
pub fn for_a_picture(mut prepared: PrintDocument) -> PrintDocument {
    prepared.style.push_str(PICTURE_STYLE);
    prepared
}

fn failed(what: &str) {
    log::warn!("no picture of a mail: {what}");
    check::failed();
    crate::account_check::failed("drawing the mail");
    go_away();
    actions::toast(toasts::FAILED);
}

fn draw(app: &App, prepared: PrintDocument, purpose: Purpose) {
    go_away();
    reached("making the page");
    let weak = app.as_weak();
    let built = crate::webviews::page(|builder| {
        crate::mail_view::keep_to_drawing(builder)
            .page_scripts_off()
            .with_navigation_handler(|url| print_view::own_page(&url))
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, PageLoadEvent::Finished) {
                    reached("page loaded");
                    let (weak, prepared) = (weak.clone(), prepared.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        if weak.upgrade().is_some() {
                            place(&prepared, purpose);
                        }
                    });
                }
            })
            .with_html(print_view::page())
            .with_incognito(true)
            .with_bounds(Rect { position: out_of_sight().into(), size: LogicalSize::new(WIDTH, 1.0).into() })
            .with_visible(!system::DRAWS_HIDDEN)
            .build_as_child(&app.window().window_handle())
    });
    match built {
        Ok(view) => {
            crate::webviews::scripts_settled(&view);
            VIEW.set(Some(view));
            reached("page made");
        }
        Err(error) => {
            log::warn!("no webview for the mail picture: {}", crate::webviews::failure(&error));
            failed("no webview");
        }
    }
}

fn place(prepared: &PrintDocument, purpose: Purpose) {
    let Some(script) = print_view::placing(prepared) else { return failed("no script") };
    VIEW.with_borrow(|view| {
        let Some(view) = view else { return };
        let done = view.evaluate_script_with_callback(&script, move |_| {
            let _ = slint::invoke_from_event_loop(move || {
                reached("mail placed");
                measure(purpose);
            });
        });
        if done.is_err() {
            failed("not placed");
        }
    });
}

fn measure(purpose: Purpose) {
    VIEW.with_borrow(|view| {
        let Some(view) = view else { return };
        let done = view.evaluate_script_with_callback(MEASURE, move |measured| {
            let (height, cut) = drawn_height(&measured);
            let _ = slint::invoke_from_event_loop(move || {
                reached("measured");
                log::debug!("the mail picture is {height:.0} points high");
                sized(height, cut, purpose);
            });
        });
        if done.is_err() {
            failed("not measured");
        }
    });
}

fn sized(height: f64, cut: bool, purpose: Purpose) {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.set_bounds(Rect {
                position: out_of_sight().into(),
                size: LogicalSize::new(WIDTH, height).into(),
            });
        }
    });
    Timer::single_shot(SETTLES, move || {
        reached("drawing asked for");
        let asked = VIEW.with_borrow(|view| {
            view.as_ref().is_some_and(|view| {
                system::capture(view, WIDTH, height, move |png| {
                    // The webview goes on a later turn of the event loop, never inside its own callback: WebView2
                    // is still in the call that brought the picture.
                    let _ = slint::invoke_from_event_loop(move || {
                        reached("drawn");
                        go_away();
                        match png {
                            Some(png) => deliver(&Picture { png, cut }, purpose),
                            None => failed("not drawn"),
                        }
                    });
                })
            })
        });
        if !asked {
            failed("no capture");
        }
    });
}

fn deliver(picture: &Picture, purpose: Purpose) {
    log::info!("a picture of a mail drawn: {} KB{}", picture.png.len() / 1024, if picture.cut { ", cut" } else { "" });
    match purpose {
        Purpose::Check => check::finish(&picture.png, picture.cut),
        Purpose::Account => {
            let size = image::ImageReader::new(std::io::Cursor::new(&picture.png)).with_guessed_format().ok();
            let (width, height) = size.and_then(|reader| reader.into_dimensions().ok()).unwrap_or((0, 0));
            crate::account_check::finish(&picture.png, width, height);
        }
        Purpose::Copy => {
            if system::copy(&picture.png) {
                actions::toast(if picture.cut { toasts::PICTURE_CUT } else { toasts::PICTURE_COPIED });
            } else {
                failed("not copied");
            }
        }
        Purpose::Share => {
            let Some(path) = folder().map(|folder| folder.join(SHARED_NAME)) else { return failed("no folder") };
            let written = path.parent().is_some_and(|folder| std::fs::create_dir_all(folder).is_ok())
                && std::fs::write(&path, &picture.png).is_ok();
            let Some(app) = actions::main_window() else { return };
            if !written || !system::share(&app, &path) {
                failed("not shared");
            } else if picture.cut {
                actions::toast(toasts::PICTURE_CUT);
            }
        }
    }
}

#[cfg(test)]
mod tests;
