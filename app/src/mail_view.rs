//! The one place a web engine is used: drawing the body of an HTML mail. It runs no scripts, loads
//! nothing, follows no link, and exists only while such a mail is open.

use crate::App;
use crate::webviews::PageScriptsOff as _;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{
    cell::{Cell, RefCell},
    time::Duration,
};
use wry::{
    PageLoadEvent, Rect, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
};

/// Run by this app, never by the mail: a fixed-width newsletter shrinks to the pane as it would on
/// a phone. Scripts in the mail itself stay off.
fn fit_script() -> String {
    let base = crate::look::mail_text();
    format!(
        "(()=>{{const r=document.documentElement,b=document.body;b.style.zoom={base};const need=r.scrollWidth,have=r.clientWidth;if(need>have)b.style.zoom=Math.max({base}*have/need,0.5);const filled=[...b.querySelectorAll('*')].slice(0,60).find(e=>{{const c=getComputedStyle(e).backgroundColor;return c!=='rgba(0, 0, 0, 0)'&&c!=='transparent'&&e.getBoundingClientRect().width>=have*.6}});r.style.background=filled?getComputedStyle(filled).backgroundColor:''}})()"
    )
}

pub const POLICY: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:";

static SERVED: crate::webviews::OnePage = crate::webviews::OnePage::new("mailpage");

/// A page that has not finished by now is shown as it stands, for a mail too heavy to wait for.
const LONGEST_WAIT: Duration = Duration::from_millis(1200);

thread_local! {
    static VIEW: RefCell<Option<WebView>> = const { RefCell::new(None) };
    /// The page goes on screen when it is done, so nobody watches it being drawn.
    static WAITING: Cell<bool> = const { Cell::new(false) };
    static PATIENCE: Timer = Timer::default();
    /// Numbers the pages, so that one that finishes late cannot be taken for the one awaited. The
    /// number travels as the page's title, which cleaned mail has no way to set.
    static PAGE: Cell<u64> = const { Cell::new(0) };
    static PLACED_WIDTH: Cell<f32> = const { Cell::new(0.0) };
    /// What the toolkit draws over the mail now: the account panel, the tour. The page sits above all of it, so it is
    /// out of sight while any is open, and comes back as it was when the last one closes.
    static COVERS: Cell<u32> = const { Cell::new(0) };
    /// Whether the page is meant to be on screen, covered or not.
    static SHOWN: Cell<bool> = const { Cell::new(false) };
}

/// Something the toolkit draws goes over the mail, or comes away again. While it is there the page shows as a picture
/// of itself, where the toolkit can draw over it: hidden outright, the mail vanished behind a panel that covers a
/// corner.
pub fn covered(over: bool) {
    let before = COVERS.get();
    let now = if over { before.saturating_add(1) } else { before.saturating_sub(1) };
    COVERS.set(now);
    if before == 0 && now > 0 {
        freeze();
    } else if before > 0 && now == 0 {
        thaw();
    }
}

/// The page swapped for its picture once the picture is there; without one, it is only hidden.
fn freeze() {
    let shown = SHOWN.get();
    let took = shown && VIEW.with_borrow(|view| view.as_ref().is_some_and(|view| snapshot(view, swap_for_picture)));
    if !took {
        hide_now();
    }
}

fn hide_now() {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.set_visible(false);
        }
    });
}

/// The picture came: shown in the page's place, and the page hidden behind it, unless the cover went meanwhile.
fn swap_for_picture(picture: Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>) {
    if COVERS.get() == 0 {
        return;
    }
    let Some(picture) = picture else {
        hide_now();
        return;
    };
    if let Some(app) = crate::actions::main_window() {
        app.set_page_picture(slint::Image::from_rgba8(picture));
        app.set_page_frozen(true);
    }
    // The page goes once the picture has been drawn under it: hidden at once, it left an empty frame, a blink.
    Timer::single_shot(DRAWN_UNDER, || {
        if COVERS.get() > 0 {
            hide_now();
        }
    });
}

/// Two frames: the picture is on screen by then.
const DRAWN_UNDER: std::time::Duration = std::time::Duration::from_millis(34);

fn thaw() {
    let on_screen = SHOWN.get();
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.set_visible(on_screen);
        }
    });
    if let Some(app) = crate::actions::main_window() {
        app.set_page_frozen(false);
        app.set_page_picture(slint::Image::default());
    }
}

/// A picture of the page as it stands, handed to `then` on the window's thread. False when none can be asked for.
#[cfg(target_os = "macos")]
fn snapshot(view: &WebView, then: fn(Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>)) -> bool {
    use objc2::{msg_send, runtime::AnyObject};
    use wry::WebViewExtMacOS;

    let page = view.webview();
    let done = block2::RcBlock::new(move |image: *mut AnyObject, _error: *mut AnyObject| {
        // Safety: WebKit hands an `NSImage` or nothing, and it lives for this call.
        let picture = unsafe { image.as_ref() }.and_then(pixels_of);
        then(picture);
    });
    // Safety: a plain call on the page's own view, on the window's thread; no configuration means the whole page.
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
const fn snapshot(_view: &WebView, _then: fn(Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>)) -> bool {
    false
}

/// The page on screen or not, as asked, unless something is over it: then only once that has gone.
fn show_page(view: &WebView, visible: bool) {
    SHOWN.set(visible);
    let _ = view.set_visible(visible && COVERS.get() == 0);
}

/// Shows for a moment before a page paints, so it is the panel's colour and not a flash of another.
fn behind_the_page(app: &App) -> (u8, u8, u8, u8) {
    let panel = app.global::<crate::Theme>().get_panel();
    (panel.red(), panel.green(), panel.blue(), panel.alpha())
}

/// Hands the next page to the protocol, and gives its address.
fn page(html: &str) -> String {
    let number = PAGE.get() + 1;
    PAGE.set(number);
    SERVED.serve(number, document(number, html))
}

fn document(number: u64, html: &str) -> String {
    format!(
        r#"<!doctype html><html><head><title>{number}</title><meta charset="utf-8"><meta http-equiv="x-dns-prefetch-control" content="off"><meta http-equiv="Content-Security-Policy" content="{POLICY}"><style>html{{background:#fff;overscroll-behavior:none}}::-webkit-scrollbar{{width:10px;height:0}}::-webkit-scrollbar-track{{background:transparent}}::-webkit-scrollbar-thumb{{background:#0003;border:2px solid transparent;background-clip:padding-box;border-radius:5px}}body{{margin:0;padding:16px 22px 24px;color:#000;font:14px system-ui,sans-serif;overflow-wrap:break-word;overscroll-behavior:none}}img{{max-width:100%;height:auto}}table{{max-width:100%!important}}td,th{{overflow-wrap:anywhere}}</style></head><body>{html}</body></html>"#
    )
}

/// The toolkit's lengths grow with the zoom, and the system's do not.
fn bounds(app: &App) -> Rect {
    let zoom = crate::look::zoom();
    Rect {
        position: LogicalPosition::new(f64::from(app.get_body_x()) * zoom, f64::from(app.get_body_y()) * zoom).into(),
        size: LogicalSize::new(f64::from(app.get_body_width()) * zoom, f64::from(app.get_body_height()) * zoom).into(),
    }
}

/// Hostile mail may yet crash the engine's process. Loading it again would only crash it again, so
/// the page goes empty and the person is told, and the mail stays one click from being tried again.
#[cfg(target_os = "macos")]
fn when_the_page_dies(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    use wry::WebViewBuilderExtDarwin;

    builder.with_on_web_content_process_terminate_handler(|| {
        log::error!("the page of a mail crashed");
        let _ = slint::invoke_from_event_loop(|| {
            close();
            crate::actions::toast(crate::toasts::INTERNAL);
        });
    })
}

#[cfg(not(target_os = "macos"))]
const fn when_the_page_dies(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder
}

/// Only the page handed over may load. A link goes to the browser, and nothing else goes at all:
/// a relative link in a mail would resolve inside the protocol, so only the exact address counts.
fn own_page(url: &str) -> bool {
    if SERVED.is_own(url) || url == "about:blank" {
        return true;
    }
    // An earlier page of the mail's own, reported late: neither loaded again nor sent to the browser, where on
    // Windows its http address opened Edge.
    if SERVED.is_ours(url) {
        log::info!("an earlier page of the mail view was left alone");
        return false;
    }
    crate::outside::open_link(url);
    false
}

/// Draws a new mail out of sight, and shows it once it is whole.
pub fn show_finished(app: &App, html: &str) {
    set_visible(false);
    WAITING.set(true);
    show(app, html);
    PATIENCE.with(|timer| timer.start(TimerMode::SingleShot, LONGEST_WAIT, reveal));
}

fn reveal() {
    PATIENCE.with(Timer::stop);
    if WAITING.replace(false) {
        VIEW.with_borrow(|view| {
            if let Some(view) = view {
                show_page(view, true);
            }
        });
    }
}

/// Fits the page first: a newsletter must not be seen at its full width and then jump.
fn fit_then_reveal() {
    let script = format!("{};document.title", fit_script());
    let fitted = VIEW.with_borrow(|view| {
        view.as_ref().is_some_and(|view| {
            view.evaluate_script_with_callback(&script, |title| {
                drop(slint::invoke_from_event_loop(move || {
                    if title.trim_matches('"') == PAGE.get().to_string() {
                        reveal();
                    }
                }));
            })
            .is_ok()
        })
    });
    if !fitted {
        reveal();
    }
}

pub fn show(app: &App, html: &str) {
    let address = page(html);
    VIEW.with_borrow_mut(|view| {
        if let Some(existing) = view.as_ref() {
            round_the_corner(existing, app);
            // The view outlives a change of theme, and its edge shows beside a page that does not fill it.
            let _ = existing.set_background_color(behind_the_page(app));
            let _ = existing.load_url(&address);
            return;
        }
        let built = crate::webviews::page(|builder| {
            when_the_page_dies(keep_to_drawing(builder))
                .page_scripts_off()
                .with_background_color(behind_the_page(app))
                .with_navigation_handler(|url| own_page(&url))
                .with_on_page_load_handler(|event, _| {
                    if matches!(event, PageLoadEvent::Finished) {
                        let _ = slint::invoke_from_event_loop(fit_then_reveal);
                    }
                })
                .with_custom_protocol(SERVED.protocol().to_string(), |_, request| SERVED.answer(&request, Some(POLICY)))
                .with_url(&address)
                .with_incognito(true)
                .with_visible(!WAITING.get() && COVERS.get() == 0)
                .with_bounds(bounds(app))
                .build_as_child(&app.window().window_handle())
        });
        match built {
            Ok(created) => {
                crate::webviews::scripts_settled(&created);
                SHOWN.set(!WAITING.get());
                round_the_corner(&created, app);
                let _ = created.zoom(crate::look::zoom());
                *view = Some(created);
            }
            Err(error) => {
                log::error!("no webview for mail: {}", crate::webviews::failure(&error));
                crate::actions::toast(crate::toasts::NO_ENGINE);
            }
        }
    });
}

/// A mail is a page to draw once: no scripts, no canvas, no video, no history to go back through,
/// and nothing kept on disk. WebKit reads these before the page exists, so they cannot be changed on
/// a page already made. Drawing itself stays WebKit's own, on the GPU: the memory that saves is not
/// worth a page that can come late. The switches are not public: each is asked about first, and
/// without them a mail is still drawn.
#[cfg(target_os = "macos")]
pub fn keep_to_drawing(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    const NOT_NEEDED: [&str; 6] = [
        "UseGPUProcessForCanvasRenderingEnabled",
        "UseGPUProcessForMediaEnabled",
        "UseGPUProcessForWebGLEnabled",
        "UsesBackForwardCache",
        "WebGPUEnabled",
        "CacheAPIEnabled",
    ];
    with_features_off(builder, &NOT_NEEDED)
}

/// For Proton's verification page, which runs its own scripts. WebRTC can send UDP to any server past the proxy,
/// and with it the real address Tor hides, so a peer connection cannot be made at all.
#[cfg(target_os = "macos")]
pub fn without_peer_connections(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    with_features_off(builder, &PEER_CONNECTIONS)
}

#[cfg(target_os = "macos")]
const PEER_CONNECTIONS: [&str; 3] =
    ["PeerConnectionEnabled", "WebRTCPlatformTCPSocketsEnabled", "WebRTCPlatformUDPSocketsEnabled"];

/// WebKitGTK has WebRTC off unless asked; WebView2 gets it from its environment's arguments, in `verify.rs`.
#[cfg(not(target_os = "macos"))]
pub const fn without_peer_connections(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder
}

/// A configuration with nothing kept on disk and the named features of WebKit off. The switches are private: each
/// is asked about first, and one that is missing is passed over.
#[cfg(target_os = "macos")]
fn with_features_off<'a>(builder: WebViewBuilder<'a>, off: &[&str]) -> WebViewBuilder<'a> {
    use objc2::{
        MainThreadMarker, msg_send,
        rc::Retained,
        runtime::{AnyClass, AnyObject, Bool},
        sel,
    };
    use objc2_foundation::{NSArray, NSString};
    use objc2_web_kit::{WKWebViewConfiguration, WKWebsiteDataStore};
    use wry::WebViewBuilderExtMacos;

    let Some(marker) = MainThreadMarker::new() else { return builder };
    let Some(kind) = AnyClass::get(c"WKPreferences") else { return builder };
    // Safety: each private selector is asked about before it is sent, and the rest is public.
    unsafe {
        let configuration = WKWebViewConfiguration::new(marker);
        configuration.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(marker));
        let preferences = configuration.preferences();
        let listed: Bool = msg_send![kind, respondsToSelector: sel!(_features)];
        let settable: Bool = msg_send![&*preferences, respondsToSelector: sel!(_setEnabled:forFeature:)];
        // Private calls may answer nothing in another version of WebKit: that is a feature left as it is, never a
        // panic, which a result that cannot be nothing would be.
        let features: Option<Retained<NSArray<AnyObject>>> =
            if listed.as_bool() && settable.as_bool() { msg_send![kind, _features] } else { None };
        for feature in features.iter().flat_map(|features| features.iter()) {
            let key: Option<Retained<NSString>> = msg_send![&*feature, key];
            if let Some(key) = key.filter(|key| off.contains(&key.to_string().as_str())) {
                let _: () = msg_send![&*preferences, _setEnabled: false, forFeature: &*feature];
                log::debug!("a WebKit feature turned off: {key}");
            }
        }
        builder.with_webview_configuration(configuration)
    }
}

#[cfg(not(target_os = "macos"))]
pub const fn keep_to_drawing(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    builder
}

/// In drafts there are no reply buttons, so the page reaches the panel's lower right corner and would
/// square it off. Anywhere else it has no corner to follow.
#[cfg(target_os = "macos")]
fn round_the_corner(view: &WebView, app: &App) {
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
const fn round_the_corner(_view: &WebView, _app: &App) {}

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

fn fit() {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.evaluate_script(&fit_script());
        }
    });
}

/// Runs in the page's own world even with scripts off: this is the app speaking, not the mail.
pub fn scroll_page(direction: i32) {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.evaluate_script(&format!("window.scrollBy(0, {direction} * window.innerHeight * 0.85)"));
        }
    });
}

/// The page grows with the rest of the window.
pub fn zoom_to(zoom: f64) {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.zoom(zoom);
        }
    });
}

/// Called on every frame of a move, and only a new width needs the page measured again.
pub fn place(app: &App) {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.set_bounds(bounds(app));
        }
    });
    let width = app.get_body_width();
    // A width in logical pixels: what matters is whether it moved, not by how little.
    if (PLACED_WIDTH.replace(width) - width).abs() > f32::EPSILON {
        fit();
    }
}

/// The webview sits above everything the toolkit draws, so a panel over the mail means hiding it.
pub fn set_visible(visible: bool) {
    WAITING.set(false);
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            show_page(view, visible);
        }
    });
}

/// Out of sight and emptied, but alive: starting WebKit again takes about a second, and it is the
/// difference between a mail that opens and one that arrives.
pub fn close() {
    set_visible(false);
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.load_url(&page(""));
        }
    });
}

/// Nothing of a mail may outlive the session.
pub fn destroy() {
    WAITING.set(false);
    VIEW.with_borrow_mut(|view| drop(view.take()));
}

/// Starts WebKit before it is needed, so that the first mail opens like any other.
pub fn warm_up(app: &App) {
    if VIEW.with_borrow(Option::is_none) {
        WAITING.set(true);
        show(app, "");
        WAITING.set(false);
    }
}

#[cfg(test)]
mod tests;
