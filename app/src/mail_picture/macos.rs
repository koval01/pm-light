//! WebKit makes a PDF of the whole page, out of sight as the page is, and AppKit draws that PDF into pixels. A
//! snapshot would hold only what a view on screen shows.

use super::{SCALE, scale_for};
use crate::App;
use objc2::{
    msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString};
use std::{cell::RefCell, path::Path};
use wry::{WebView, WebViewExtMacOS};

pub const CAN_SHARE: bool = true;
/// WebKit lays out and prints a page that is not shown.
pub const DRAWS_HIDDEN: bool = true;
/// `NSBitmapImageFileTypePNG`.
const PNG: usize = 4;
/// `NSRectEdge`: the menu opens below its spot, which is the larger y in a flipped view.
const MIN_Y: usize = 1;
const MAX_Y: usize = 3;
/// The spot, from the mail's top right corner: below the toolbar's More, where the menu was asked for.
const FROM_RIGHT: f64 = 60.0;

thread_local! {
    /// The menu for sharing, held while it is open: AppKit does not hold it.
    static PICKER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
}

fn class(name: &std::ffi::CStr) -> Option<&'static AnyClass> {
    AnyClass::get(name)
}

/// The page as PNG, handed to `then` on the window's thread. False when it cannot be asked for.
pub fn capture(view: &WebView, _width: f64, _height: f64, then: impl FnOnce(Option<Vec<u8>>) + 'static) -> bool {
    let page = view.webview();
    let then = RefCell::new(Some(then));
    let done = block2::RcBlock::new(move |data: *mut NSData, _error: *mut AnyObject| {
        // Safety: WebKit hands its data or nothing, alive for this call.
        let png = unsafe { data.as_ref() }.and_then(drawn);
        if let Some(then) = then.borrow_mut().take() {
            then(png);
        }
    });
    // Safety: a plain call on the page's own view, on the window's thread. No configuration is the whole page.
    unsafe {
        let none: Option<&AnyObject> = None;
        let _: () = msg_send![&*page, createPDFWithConfiguration: none, completionHandler: &*done];
    }
    true
}

/// The first page of a PDF drawn into pixels, on white, at the scale its size allows. Nothing for what is not one.
pub fn drawn(pdf: &NSData) -> Option<Vec<u8>> {
    // Every class first: a missing one must end this before the graphics state is changed, never between its save
    // and its restore.
    let (reps, bitmaps, contexts, colors, paths) =
        (class(c"NSPDFImageRep")?, class(c"NSBitmapImageRep")?, class(c"NSGraphicsContext")?, class(c"NSColor")?, class(c"NSBezierPath")?);
    // Safety: AppKit's own classes, each result checked for nothing before it is used. The context drawn into is a
    // bitmap's own, made current for this thread only and put back before the block ends.
    unsafe {
        let rep: Option<Retained<AnyObject>> = msg_send![reps, imageRepWithData: pdf];
        let rep = rep?;
        let pages: isize = msg_send![&*rep, pageCount];
        let bounds: NSRect = msg_send![&*rep, bounds];
        let (width, height) = (bounds.size.width, bounds.size.height);
        if pages < 1 || !(width >= 1.0 && height >= 1.0) {
            return None;
        }
        let scale = scale_for(width, height);
        let (wide, high) = (pixels(width * scale)?, pixels(height * scale)?);
        let allocated: Allocated<AnyObject> = msg_send![bitmaps, alloc];
        let planes: *mut *mut u8 = std::ptr::null_mut();
        let space = NSString::from_str("NSDeviceRGBColorSpace");
        let bitmap: Option<Retained<AnyObject>> = msg_send![
            allocated,
            initWithBitmapDataPlanes: planes,
            pixelsWide: wide,
            pixelsHigh: high,
            bitsPerSample: 8_isize,
            samplesPerPixel: 4_isize,
            hasAlpha: Bool::YES,
            isPlanar: Bool::NO,
            colorSpaceName: &*space,
            bytesPerRow: 0_isize,
            bitsPerPixel: 0_isize
        ];
        let bitmap = bitmap?;
        let _: () = msg_send![&*bitmap, setSize: NSSize::new(width, height)];
        let context: Option<Retained<AnyObject>> = msg_send![contexts, graphicsContextWithBitmapImageRep: &*bitmap];
        let context = context?;
        let whole = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
        let _: () = msg_send![contexts, saveGraphicsState];
        let _: () = msg_send![contexts, setCurrentContext: &*context];
        let white: Option<Retained<AnyObject>> = msg_send![colors, whiteColor];
        if let Some(white) = white {
            let _: () = msg_send![&*white, setFill];
            let _: () = msg_send![paths, fillRect: whole];
        }
        let _: Bool = msg_send![&*rep, drawInRect: whole];
        let _: () = msg_send![&*context, flushGraphics];
        let _: () = msg_send![contexts, restoreGraphicsState];
        let properties = NSDictionary::<AnyObject, AnyObject>::new();
        let png: Option<Retained<NSData>> = msg_send![&*bitmap, representationUsingType: PNG, properties: &*properties];
        Some(png?.to_vec())
    }
}

fn pixels(points: f64) -> Option<isize> {
    let rounded = points.round();
    // Within `TALLEST` times `SCALE`, far inside an `isize`.
    (1.0..=super::TALLEST * SCALE).contains(&rounded).then(|| format!("{rounded:.0}").parse().ok()).flatten()
}

fn data_of(bytes: &[u8]) -> Retained<NSData> {
    NSData::with_bytes(bytes)
}

/// Onto the clipboard as PNG. A program that reads only TIFF gets one from the pasteboard itself.
pub fn copy(png: &[u8]) -> bool {
    let png = data_of(png);
    // Safety: AppKit's own pasteboard, on the window's thread.
    unsafe {
        let Some(board): Option<Retained<AnyObject>> = class(c"NSPasteboard").and_then(|boards| msg_send![boards, generalPasteboard]) else {
            return false;
        };
        put(&board, &png)
    }
}

/// # Safety
///
/// `board` is an `NSPasteboard`.
unsafe fn put(board: &AnyObject, png: &NSData) -> bool {
    // Safety: the caller hands a pasteboard, and both are plain calls on it with data and a type it takes.
    unsafe {
        let _: isize = msg_send![board, clearContents];
        let placed: Bool = msg_send![board, setData: png, forType: &*NSString::from_str("public.png")];
        placed.as_bool()
    }
}

/// For the check: onto a pasteboard of its own, never the person's, and back as it went on.
pub fn copy_checked(png: &[u8]) -> bool {
    put_and_read_back(png).is_some_and(|(read, _)| read == png)
}

/// What a pasteboard of its own holds as PNG, and whether it offers TIFF too: the general one is the person's.
pub fn put_and_read_back(png: &[u8]) -> Option<(Vec<u8>, bool)> {
    let data = data_of(png);
    // Safety: a pasteboard made for this call alone, released at its end.
    unsafe {
        let board: Retained<AnyObject> = msg_send![class(c"NSPasteboard")?, pasteboardWithUniqueName];
        let placed = put(&board, &data);
        let read: Option<Retained<NSData>> = msg_send![&*board, dataForType: &*NSString::from_str("public.png")];
        let tiff: Option<Retained<NSData>> = msg_send![&*board, dataForType: &*NSString::from_str("public.tiff")];
        let _: () = msg_send![&*board, releaseGlobally];
        placed.then(|| read.map(|read| (read.to_vec(), tiff.is_some()))).flatten()
    }
}

/// The system's menu for sharing, with the picture's file, below the mail's toolbar.
pub fn share(app: &App, path: &Path) -> bool {
    use slint::ComponentHandle;
    use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = app.window().window_handle();
    let Ok(handle) = handle.window_handle() else { return false };
    let RawWindowHandle::AppKit(view) = handle.as_raw() else { return false };
    let Some(path) = path.to_str() else { return false };
    // Safety: the window's own view, alive while the window is, and AppKit's classes, on the window's thread.
    unsafe {
        let view: &AnyObject = view.ns_view.cast::<AnyObject>().as_ref();
        let Some(classes) = class(c"NSURL").zip(class(c"NSArray")).zip(class(c"NSSharingServicePicker")) else {
            return false;
        };
        let ((urls, arrays), pickers) = classes;
        let url: Option<Retained<AnyObject>> = msg_send![urls, fileURLWithPath: &*NSString::from_str(path)];
        let Some(url) = url else { return false };
        let items: Retained<AnyObject> = msg_send![arrays, arrayWithObject: &*url];
        let allocated: Allocated<AnyObject> = msg_send![pickers, alloc];
        let picker: Option<Retained<AnyObject>> = msg_send![allocated, initWithItems: &*items];
        let Some(picker) = picker else { return false };
        let flipped: Bool = msg_send![view, isFlipped];
        let bounds: NSRect = msg_send![view, bounds];
        let (x, y) = (f64::from(app.get_body_x() + app.get_body_width()) - FROM_RIGHT, f64::from(app.get_body_y()));
        let (at, edge) = if flipped.as_bool() { (y, MAX_Y) } else { (bounds.size.height - y, MIN_Y) };
        let spot = NSRect::new(NSPoint::new(x.max(0.0), at), NSSize::new(1.0, 1.0));
        let _: () = msg_send![&*picker, showRelativeToRect: spot, ofView: view, preferredEdge: edge];
        PICKER.set(Some(picker));
    }
    true
}
