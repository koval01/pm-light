//! Where webviews keep what they must write. With no folder given, WebView2 makes one beside the
//! program, which cannot be written under Program Files, so no mail would open, and would leave its
//! cache outside what quitting erases. Both folders live in the cache folder `guard` empties.
//!
//! WebView2 shares a folder only between webviews made with the same environment options. Ours all
//! use wry's defaults, and Proton's page gets a folder of its own anyway, so that its cookies never
//! sit beside a mail.
//!
//! On Linux a context is where protocols are registered, once each. An incognito webview gets a
//! context of its own there whatever is passed, so a webview with a protocol of its own must be
//! incognito, or the second one made would be refused.

use parking_lot::Mutex;
use std::{borrow::Cow, cell::RefCell, path::PathBuf, sync::Arc};
use wry::{
    NewWindowResponse, ProxyConfig, ProxyEndpoint, Rect, WebContext, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize},
    http::{Request, Response, StatusCode, header},
};

thread_local! {
    static PAGES: RefCell<WebContext> = RefCell::new(WebContext::new(folder("pages")));
    static PROTON: RefCell<WebContext> = RefCell::new(WebContext::new(folder("proton")));
}

fn folder(name: &str) -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join(crate::guard::IDENTIFIER).join("webview").join(name))
}

fn with_context<T>(
    context: &'static std::thread::LocalKey<RefCell<WebContext>>,
    make: impl FnOnce(WebViewBuilder<'_>) -> T,
) -> T {
    context.with(|context| {
        if let Ok(mut context) = context.try_borrow_mut() {
            make(WebViewBuilder::new_with_web_context(&mut context))
        } else {
            log::warn!("a webview was made without its data folder");
            make(WebViewBuilder::new())
        }
    })
}

/// For the app's own pages: mail, the editor, previews and printouts. None of them needs the network, so each is cut off
/// from it: every connection it makes goes to the door to nowhere (`nowhere.rs`), and it opens no window. A page that
/// hands its links to the browser says so after this, in `make`.
pub fn page<T>(make: impl FnOnce(WebViewBuilder<'_>) -> T) -> T {
    with_context(&PAGES, |builder| make(cut_off(builder)))
}

fn cut_off(builder: WebViewBuilder<'_>) -> WebViewBuilder<'_> {
    let builder = builder.with_new_window_req_handler(|_, _| NewWindowResponse::Deny);
    let Some(door) = crate::nowhere::door().filter(|_| proxies_taken()) else { return builder };
    let endpoint = ProxyEndpoint { host: door.ip().to_string(), port: door.port().to_string() };
    builder.with_proxy_config(ProxyConfig::Socks5(endpoint))
}

/// WebKit on macOS takes a proxy from macOS 14 on. Before it, a mail's page has only its own switches against
/// reaching out (`switches::keep_to_drawing`), and the journal says so once.
#[cfg(target_os = "macos")]
fn proxies_taken() -> bool {
    use objc2_foundation::NSProcessInfo;
    static SAID: std::sync::Once = std::sync::Once::new();
    let taken = NSProcessInfo::processInfo().operatingSystemVersion().majorVersion >= 14;
    if !taken {
        SAID.call_once(|| log::info!("the app's pages go without the door to nowhere: macOS before 14 takes no proxy"));
    }
    taken
}

#[cfg(not(target_os = "macos"))]
const fn proxies_taken() -> bool {
    true
}

/// The page's own scripts off. Elsewhere that is all of JavaScript, and the app's calls into the page, which fit a mail
/// and place a printout, still run in a world of their own. WebKitGTK's switch for it stops those calls too, so on
/// Linux only the scripts of the page's markup go, by `scripts_settled` once the webview exists. Until then the page's
/// policy (`default-src 'none'`) already forbids every script.
pub trait PageScriptsOff {
    #[must_use]
    fn page_scripts_off(self) -> Self;
}

impl PageScriptsOff for WebViewBuilder<'_> {
    fn page_scripts_off(self) -> Self {
        #[cfg(target_os = "linux")]
        {
            self
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.with_javascript_disabled()
        }
    }
}

/// Right after a webview made with `page_scripts_off` exists.
#[cfg(target_os = "linux")]
pub fn scripts_settled(view: &wry::WebView) {
    use webkit2gtk::{SettingsExt as _, WebViewExt as _};
    use wry::WebViewExtUnix as _;

    if let Some(settings) = view.webview().settings() {
        settings.set_enable_javascript_markup(false);
    }
}

#[cfg(not(target_os = "linux"))]
pub const fn scripts_settled(_view: &wry::WebView) {}

/// The most a webview spans each way, in logical pixels. X11 keeps a window's size in 16 bits, so a larger one
/// would wrap, and twice this still fits at a scale of two.
const LARGEST: f64 = 16_384.0;

/// Webviews whose last place had no room, so that the journal says so once and not on every frame of a move.
static SHRUNK: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// A webview the app keeps, which says in the journal when it goes. On Linux it goes inside GDK's own trap for X11
/// errors: wry destroys its X11 window under GTK's widgets, and GTK, destroying theirs again in its loop, is refused
/// with a `BadWindow` on `DestroyWindow`, which GDK otherwise ends the program over. A refusal is written down, not
/// hidden. Every webview kept is one of these (`app/tests/structure.rs`).
pub struct Held {
    view: wry::WebView,
    /// Goes after `view`, since fields go in the order they are written: it lifts the trap once the view is gone.
    gone: Gone,
}

impl Held {
    /// `what` is a fixed word for the journal.
    pub const fn new(what: &'static str, view: wry::WebView) -> Self {
        Self { view, gone: Gone { what } }
    }
}

impl std::ops::Deref for Held {
    type Target = wry::WebView;

    fn deref(&self) -> &wry::WebView {
        &self.view
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        log::debug!("the webview for {} goes", self.gone.what);
        #[cfg(target_os = "linux")]
        if let Some(display) = x11_display() {
            display.error_trap_push();
        }
    }
}

struct Gone {
    what: &'static str,
}

impl Drop for Gone {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        lift_x11_trap(self.what);
        log::debug!("the webview for {} is gone", self.what);
    }
}

#[cfg(target_os = "linux")]
fn x11_display() -> Option<gdkx11::X11Display> {
    use gtk::prelude::Cast as _;
    gtk::gdk::Display::default()?.downcast::<gdkx11::X11Display>().ok()
}

/// GTK's loop runs first, so that what GTK does about the webview going happens inside the trap, and the server is
/// asked to answer everything before the trap comes off.
#[cfg(target_os = "linux")]
fn lift_x11_trap(what: &'static str) {
    use gtk::prelude::Cast as _;
    let Some(display) = x11_display() else { return };
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
    display.upcast_ref::<gtk::gdk::Display>().sync();
    let refused = display.error_trap_pop();
    if refused != 0 {
        log::info!("X11 refused a call while the webview for {what} went: error {refused}, after the window was gone");
    }
}

/// The size WebKit laid the page out at, in logical pixels, where the system lets the app ask: on Linux, GTK's
/// allocation of the page, which a check compares with the window wry gave it.
#[cfg(target_os = "linux")]
pub fn laid_out(view: &wry::WebView) -> Option<(i32, i32)> {
    use gtk::prelude::WidgetExt as _;
    use wry::WebViewExtUnix as _;
    let page = view.webview();
    Some((page.allocated_width(), page.allocated_height()))
}

#[cfg(not(target_os = "linux"))]
pub const fn laid_out(_view: &wry::WebView) -> Option<(i32, i32)> {
    None
}

/// Gives GTK's own window, which wry puts the page in, the size its X11 window has, in GTK's own way.
#[cfg(target_os = "linux")]
pub fn tell_gtk_the_size(view: &wry::WebView, width: i32, height: i32) {
    use gtk::prelude::{Cast as _, GtkWindowExt as _, WidgetExt as _};
    use wry::WebViewExtUnix as _;
    if let Some(window) = view.webview().toplevel().and_then(|top| top.downcast::<gtk::Window>().ok()) {
        window.resize(width.max(1), height.max(1));
    }
}

#[cfg(not(target_os = "linux"))]
pub const fn tell_gtk_the_size(_view: &wry::WebView, _width: i32, _height: i32) {}

/// Notes whether `what` has room now, and answers whether it has just lost it.
fn newly_squeezed(what: &'static str, squeezed: bool) -> bool {
    let mut shrunk = SHRUNK.lock();
    let known = shrunk.contains(&what);
    if squeezed && !known {
        shrunk.push(what);
    } else if !squeezed && known {
        shrunk.retain(|other| *other != what);
    }
    drop(shrunk);
    squeezed && !known
}

/// Where a webview sits, at least a pixel each way. X11 refuses a window with no width or height, and GTK ends
/// the program over it, with a `BadValue` on `CreateWindow`: wry makes such a window for every webview, of the
/// size it is given. A layout not yet measured, or a pane squeezed to nothing, gives just that. `what` is a fixed
/// word for the journal.
pub fn area(what: &'static str, x: f64, y: f64, width: f64, height: f64) -> Rect {
    let room = |length: f64| if length.is_nan() { 1.0 } else { length.clamp(1.0, LARGEST) };
    let (fitted_width, fitted_height) = (room(width), room(height));
    let squeezed = width.is_nan() || height.is_nan() || width < 1.0 || height < 1.0;
    if newly_squeezed(what, squeezed) {
        log::debug!("the webview for {what} was given {width:.1} by {height:.1}, and takes a pixel at least");
    }
    Rect { position: LogicalPosition::new(x, y).into(), size: LogicalSize::new(fitted_width, fitted_height).into() }
}

/// For Proton's verification page.
pub fn proton<T>(make: impl FnOnce(WebViewBuilder<'_>) -> T) -> T {
    with_context(&PROTON, make)
}

/// What went wrong, safe for the journal: an error's own text may hold a path.
pub fn failure(error: &wry::Error) -> String {
    match error {
        #[cfg(target_os = "windows")]
        wry::Error::WebView2Error(webview2_com::Error::WindowsError(inner)) => {
            format!("WebView2 {:#010x}", inner.code().0)
        }
        #[cfg(target_os = "windows")]
        wry::Error::WebView2Error(_) => "WebView2".to_string(),
        wry::Error::Io(inner) => format!("io {:?}", inner.kind()),
        wry::Error::WindowHandleError(_) | wry::Error::UnsupportedWindowHandle => "window handle".to_string(),
        wry::Error::NotMainThread => "not the main thread".to_string(),
        _ => "other".to_string(),
    }
}

/// One page at a time, served by a protocol of the webview's own instead of `with_html`: WebView2
/// takes 2 MB at most that way, and a mail with its pictures inside is often more. Every system goes
/// the same way, so that what Windows does is what macOS shows every day. The protocol answers the
/// page's one address and nothing else, and a webview using it must be incognito: see above.
pub struct OnePage {
    protocol: &'static str,
    served: Mutex<Option<(String, Arc<Vec<u8>>)>>,
}

impl OnePage {
    pub const fn new(protocol: &'static str) -> Self {
        Self { protocol, served: Mutex::new(None) }
    }

    pub const fn protocol(&self) -> &'static str {
        self.protocol
    }

    /// Wry writes a protocol of the app's own as an http address on Windows.
    fn origin(&self) -> String {
        if cfg!(target_os = "windows") {
            format!("http://{}.page", self.protocol)
        } else {
            format!("{}://page", self.protocol)
        }
    }

    /// Hands the page over, and gives the address to load it from. A new number each time, so that
    /// nothing cached can stand in for it.
    pub fn serve(&self, number: u64, page: String) -> String {
        let address = format!("{}/{number}", self.origin());
        *self.served.lock() = Some((address.clone(), Arc::new(page.into_bytes())));
        address
    }

    /// For the navigation handler: only the exact address. A relative link in a mail would resolve
    /// inside the protocol, so a prefix would not do.
    pub fn is_own(&self, url: &str) -> bool {
        self.served.lock().as_ref().is_some_and(|(address, _)| address == url)
    }

    /// Any page this protocol served, the one now or an earlier one. A webview can still report an earlier page after
    /// a newer one was handed over, when two mails open one right after the other: that is no link to send out.
    pub fn is_ours(&self, url: &str) -> bool {
        url.strip_prefix(&self.origin()).is_some_and(|rest| rest.starts_with('/'))
    }

    /// By path: on Windows wry turns the address into an http one and back, and only the path stays.
    /// `policy` goes out as a header. A page whose policy carries a fresh nonce keeps it in its own
    /// `<meta>` and passes `None`: a header without the nonce would block its script.
    pub fn answer(&self, request: &Request<Vec<u8>>, policy: Option<&str>) -> Response<Cow<'static, [u8]>> {
        let asked = request.uri().path();
        let served = self.served.lock().clone();
        let built = match served {
            Some((address, page)) if asked.len() > 1 && address.ends_with(asked) => {
                let builder = Response::builder()
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
                let builder = match policy {
                    Some(policy) => builder.header(header::CONTENT_SECURITY_POLICY, policy),
                    None => builder,
                };
                builder.body(Cow::Owned(page.as_ref().clone()))
            }
            _ => Response::builder().status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&[][..])),
        };
        built.unwrap_or_else(|_| Response::new(Cow::Borrowed(&[][..])))
    }
}

#[cfg(test)]
mod tests;
