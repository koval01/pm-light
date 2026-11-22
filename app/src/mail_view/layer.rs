//! What WebKit's layer does for the mail's page on macOS: its corners follow the panel's, and a picture of it stands in
//! its place while something the toolkit draws covers it. Elsewhere neither is done.

use crate::App;
use wry::WebView;

/// A picture of the page as it stands, handed to `then` on the window's thread. False when none can be asked for.
#[cfg(target_os = "macos")]
pub(super) fn snapshot(view: &WebView, then: fn(Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>)) -> bool {
    use objc2::{msg_send, runtime::AnyObject};
    use wry::WebViewExtMacOS;

    let page = view.webview();
    let done = block2::RcBlock::new(move |image: *mut AnyObject, _error: *mut AnyObject| {
        // Safety: WebKit hands an `NSImage` or nothing, and it lives for this call.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let picture = unsafe { image.as_ref() }.and_then(pixels_of);
        then(picture);
    });
    // Safety: a plain call on the page's own view, on the window's thread; no configuration means the whole page.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        let none: Option<&AnyObject> = None;
        let _: () = msg_send![&*page, takeSnapshotWithConfiguration: none, completionHandler: &*done];
    }
    true
}

/// An `NSImage` as pixels: through PNG, which every image of AppKit can become.
#[cfg(target_os = "macos")]
fn pixels_of(image: &objc2::runtime::AnyObject) -> Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>> {
    use objc2::{
        msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject},
    };
    use objc2_foundation::{NSData, NSDictionary};

    // `NSBitmapImageFileTypePNG`.
    const PNG: usize = 4;
    // Safety: plain AppKit calls on an image WebKit made, each checked for nothing before it is used.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let png: Retained<NSData> = unsafe {
        let tiff: Option<Retained<NSData>> = msg_send![image, TIFFRepresentation];
        let class = AnyClass::get(c"NSBitmapImageRep")?;
        let rep: Option<Retained<AnyObject>> = msg_send![class, imageRepWithData: &*tiff?];
        let properties = NSDictionary::<AnyObject, AnyObject>::new();
        let png: Option<Retained<NSData>> = msg_send![&*rep?, representationUsingType: PNG, properties: &*properties];
        png?
    };
    let decoded = image::load_from_memory(&png.to_vec()).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    Some(slint::SharedPixelBuffer::clone_from_slice(decoded.as_raw(), width, height))
}

#[cfg(not(target_os = "macos"))]
pub(super) const fn snapshot(_view: &WebView, _then: fn(Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>)) -> bool {
    false
}

/// In drafts there are no reply buttons, so the page reaches the panel's lower right corner and would
/// square it off. Anywhere else it has no corner to follow.
#[cfg(target_os = "macos")]
pub(super) fn round_the_corner(view: &WebView, app: &App) {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::NSView;
    use wry::WebViewExtMacOS;

    const RADIUS: f64 = 14.0;
    // `kCALayerMaxXMaxYCorner`: a webview's layer is flipped, so its largest Y is the bottom.
    const LOWER_RIGHT: usize = 1 << 3;
    let radius = if app.get_folder_id() == "drafts" { RADIUS } else { 0.0 };
    let page = view.webview();
    let page: &NSView = &page;
    page.setWantsLayer(true);
    // Safety: a view that wants a layer has one, and these are plain properties of it.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![page, layer];
        if let Some(layer) = layer {
            let _: () = msg_send![&*layer, setCornerRadius: radius];
            let _: () = msg_send![&*layer, setMaskedCorners: LOWER_RIGHT];
            let _: () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) const fn round_the_corner(_view: &WebView, _app: &App) {}

/// A page that reaches the bottom of a sheet would square off its corners.
#[cfg(target_os = "macos")]
pub fn round_lower_corners(view: &WebView, radius: f64) {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::NSView;
    use wry::WebViewExtMacOS;

    // `kCALayerMinXMaxYCorner` and `kCALayerMaxXMaxYCorner`, as in `round_the_corner`.
    const LOWER: usize = (1 << 2) | (1 << 3);
    let page = view.webview();
    let page: &NSView = &page;
    page.setWantsLayer(true);
    // Safety: a view that wants a layer has one, and these are plain properties of it.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![page, layer];
        if let Some(layer) = layer {
            let _: () = msg_send![&*layer, setCornerRadius: radius];
            let _: () = msg_send![&*layer, setMaskedCorners: LOWER];
            let _: () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub const fn round_lower_corners(_view: &WebView, _radius: f64) {}
