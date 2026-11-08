//! WebKitGTK draws the whole document into a cairo surface, and the GTK clipboard takes it as a picture. There is no
//! one menu for sharing on Linux.

use super::{kept_rows, rgba_of_argb32};
use crate::App;
use gtk::{
    cairo::ImageSurface,
    gdk,
    gdk_pixbuf::{Colorspace, Pixbuf},
    glib,
};
use std::{cell::RefCell, io::Cursor, path::Path};
use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt as _};
use wry::{WebView, WebViewExtUnix};

pub const CAN_SHARE: bool = false;
/// A hidden WebKitGTK page is never laid out, and its snapshot came back 200 by 1 pixels: it is shown out of sight.
pub const DRAWS_HIDDEN: bool = false;

/// The page as PNG, handed to `then` on the window's thread. False when it cannot be asked for.
pub fn capture(view: &WebView, width: f64, height: f64, then: impl FnOnce(Option<Vec<u8>>) + 'static) -> bool {
    let page = view.webview();
    let none: Option<&gtk::gio::Cancellable> = None;
    let then = RefCell::new(Some(then));
    page.snapshot(SnapshotRegion::FullDocument, SnapshotOptions::NONE, none, move |drawn| {
        let png = drawn.ok().and_then(|surface| ImageSurface::try_from(surface).ok()).and_then(|surface| {
            let (wide, high, stride) = (surface.width(), surface.height(), surface.stride());
            let rows = kept_rows(high, wide, width, height);
            let mut picture = None;
            let _ = surface.with_data(|data| picture = rgba_of_argb32(data, wide, rows, stride));
            let mut png = Vec::new();
            picture?.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
            Some(png)
        });
        if let Some(then) = then.borrow_mut().take() {
            then(png);
        }
    });
    true
}

/// For the check, on CI's machine: the system's clipboard.
pub fn copy_checked(png: &[u8]) -> bool {
    copy(png)
}

pub fn copy(png: &[u8]) -> bool {
    let Ok(picture) = image::load_from_memory(png).map(|picture| picture.to_rgba8()) else { return false };
    let (width, height) = picture.dimensions();
    let (Ok(wide), Ok(high)) = (i32::try_from(width), i32::try_from(height)) else { return false };
    let Some(row) = wide.checked_mul(4) else { return false };
    let bytes = glib::Bytes::from_owned(picture.into_raw());
    let pixbuf = Pixbuf::from_bytes(&bytes, Colorspace::Rgb, true, 8, wide, high, row);
    gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD).set_image(&pixbuf);
    true
}

pub const fn share(_app: &App, _path: &Path) -> bool {
    false
}
