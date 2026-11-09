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
    WebContext, WebViewBuilder,
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
    context.with(|context| if let Ok(mut context) = context.try_borrow_mut() { make(WebViewBuilder::new_with_web_context(&mut context)) } else {
        log::warn!("a webview was made without its data folder");
        make(WebViewBuilder::new())
    })
}

/// For the app's own pages: mail, the editor, previews and printouts.
pub fn page<T>(make: impl FnOnce(WebViewBuilder<'_>) -> T) -> T {
    with_context(&PAGES, make)
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
        if cfg!(target_os = "windows") { format!("http://{}.page", self.protocol) } else { format!("{}://page", self.protocol) }
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
