//! WebView2 draws the whole page through its `DevTools` protocol, out of sight as the page is, and the clipboard gets it
//! as PNG and as a device-independent bitmap, for the programs that read only that. Windows' own menu for sharing is
//! not used yet.

use super::dib;
use crate::App;
use std::path::Path;
use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use windows_core::HSTRING;
use windows_sys::Win32::{
    Foundation::GlobalFree,
    System::{
        DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData},
        Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
        Ole::CF_DIB,
    },
};
use wry::{WebView, WebViewExtWindows};

pub const CAN_SHARE: bool = false;
/// Hidden, its `DevTools` capture never answered on CI: the page is shown past the window's edge instead.
pub const DRAWS_HIDDEN: bool = false;

/// What `DevTools` is asked: the page beyond what a view shows, twice as fine, as PNG.
fn asked(width: f64, height: f64) -> String {
    let scale = super::scale_for(width, height);
    format!(
        r#"{{"format":"png","captureBeyondViewport":true,"fromSurface":true,"clip":{{"x":0,"y":0,"width":{width},"height":{height},"scale":{scale}}}}}"#
    )
}

/// The page as PNG, handed to `then` on the window's thread. False when it cannot be asked for.
pub fn capture(view: &WebView, width: f64, height: f64, then: impl FnOnce(Option<Vec<u8>>) + 'static) -> bool {
    let page = view.webview();
    let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, answer: String| {
        // The HRESULT alone: a number, never the error's text.
        if let Some(code) = result.as_ref().err().map(|error| error.code().0) {
            log::warn!("DevTools did not draw the page: {code:#010x}");
        }
        let png = result.ok().and_then(|()| super::png_of_devtools(&answer));
        log::debug!("DevTools answered with {} bytes, a picture: {}", answer.len(), png.is_some());
        then(png);
        Ok(())
    }));
    // Safety: a call on the page's own interface, on the window's thread that made it.
    let asked = unsafe {
        page.CallDevToolsProtocolMethod(&HSTRING::from("Page.captureScreenshot"), &HSTRING::from(asked(width, height)), &handler)
    };
    if let Some(code) = asked.as_ref().err().map(|error| error.code().0) {
        log::warn!("DevTools could not be asked: {code:#010x}");
    }
    asked.is_ok()
}

/// Copies `bytes` into memory the clipboard can own, under `format`. True once the clipboard has it.
///
/// Safety: the clipboard is open, on this thread.
unsafe fn place(format: u32, bytes: &[u8]) -> bool {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if memory.is_null() {
            return false;
        }
        let target = GlobalLock(memory).cast::<u8>();
        if target.is_null() {
            GlobalFree(memory);
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        GlobalUnlock(memory);
        if SetClipboardData(format, memory).is_null() {
            // Not taken: still ours to free.
            GlobalFree(memory);
            return false;
        }
        true
    }
}

/// For the check, on CI's machine: the system's clipboard.
pub fn copy_checked(png: &[u8]) -> bool {
    copy(png)
}

pub fn copy(png: &[u8]) -> bool {
    let Some(bitmap) = image::load_from_memory(png).ok().map(|picture| dib(&picture.to_rgba8())) else { return false };
    let name: Vec<u16> = "PNG".encode_utf16().chain(Some(0)).collect();
    // Safety: the clipboard is opened, filled and closed on this thread; each handle is checked.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let png_format = RegisterClipboardFormatW(name.as_ptr());
        let as_png = png_format != 0 && place(png_format, png);
        let as_bitmap = place(u32::from(CF_DIB), &bitmap);
        CloseClipboard();
        as_png || as_bitmap
    }
}

pub const fn share(_app: &App, _path: &Path) -> bool {
    false
}
