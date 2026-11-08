//! Proton forbids framing its verification page, so it gets a webview of its own over the sign-in
//! screen. That webview runs Proton's full page, unchanged: a person passes the check there, and
//! this code only learns the outcome. Protection is never bypassed, and nothing here can reach it.

use crate::{App, Auth};
use slint::ComponentHandle;
use std::cell::RefCell;
use url::Url;
use wry::{
    ProxyConfig, ProxyEndpoint, WebViewBuilder,
    PageLoadEvent, Rect, WebView,
    dpi::{LogicalPosition, LogicalSize},
};

const PAGE_HOST: &str = "verify.proton.me";
const OUTCOME_SCHEME: &str = "proton-mail-light-verify";
/// `ThemeTypes.Carbon` in Proton's sources.
const DARK_THEME: &str = "1";

/// Proton's page reports to its parent, and at the top of a webview that is the page itself. The
/// webview has no way to call Rust, so the outcome leaves as a navigation that `outcome_of` stops.
const RELAY: &str = r#"
// On WebView2 the page sees `chrome.webview`, takes it for a host of Proton's own, and sends nothing.
if (window.chrome?.webview) {
  try {
    Object.defineProperty(window.chrome, "webview", { value: undefined });
  } catch {
    Object.defineProperty(window, "chrome", { value: undefined });
  }
}
addEventListener("message", (event) => {
  if (event.source !== window || typeof event.data !== "string") return;
  let message;
  try {
    message = JSON.parse(event.data);
  } catch {
    return;
  }
  if (message.type === "CLOSE") location.href = "proton-mail-light-verify://closed";
  if (message.type === "HUMAN_VERIFICATION_SUCCESS") {
    const { token, type } = message.payload;
    location.href = "proton-mail-light-verify://solved?" + new URLSearchParams({ token, type });
  }
});
"#;

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `None` for a captcha: the full page already redeemed it against the challenge's own token,
    /// which the sign-in sends again. Other methods hand back a token and its kind to use instead.
    Solved(Option<(String, String)>),
    Closed,
}

fn outcome_of(url: &Url) -> Option<Outcome> {
    if url.scheme() != OUTCOME_SCHEME {
        return None;
    }
    if url.host_str() == Some("closed") {
        return Some(Outcome::Closed);
    }
    let value = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());
    let kind = value("type").unwrap_or_default();
    let solved = (kind != "captcha").then(|| value("token").map(|token| (token, kind))).flatten();
    Some(Outcome::Solved(solved))
}

/// The address Proton gave, and only if it is Proton's own page.
pub fn page_url(address: &str, locale: &str, dark: bool) -> Option<Url> {
    let mut url = Url::parse(address).ok()?;
    if url.scheme() != "https" || url.host_str() != Some(PAGE_HOST) {
        return None;
    }
    url.query_pairs_mut().append_pair("locale", locale);
    if dark {
        url.query_pairs_mut().append_pair("theme", DARK_THEME);
    }
    Some(url)
}

/// This check sees frames too, and the captcha inside the page brings frames from other hosts.
/// Nothing here can reach the app, so it only keeps out schemes that would start other programs.
fn may_load(url: &Url) -> bool {
    matches!(url.scheme(), "https" | "about" | "blob" | "data")
}

/// There is no address bar here, so a page that left Proton could pass for the app.
fn is_proton(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(|host| host == "proton.me" || host.ends_with(".proton.me"))
}

thread_local! {
    static VIEW: RefCell<Option<WebView>> = const { RefCell::new(None) };
}

fn bounds(app: &App) -> Rect {
    let auth = app.global::<Auth>();
    Rect {
        position: LogicalPosition::new(f64::from(auth.get_verify_x()), f64::from(auth.get_verify_y())).into(),
        size: LogicalSize::new(f64::from(auth.get_verify_width()), f64::from(auth.get_verify_height())).into(),
    }
}

/// `report` gets the outcome on the window's thread, however the page ended.
pub fn open(app: &App, page: &Url, report: impl Fn(Outcome) + Clone + Send + 'static) -> bool {
    close();
    // Under Tor the page goes through Tor too, or it does not open: never by the system's own way.
    let door = if crate::tor_link::chosen() {
        let Some(tor) = crate::tor_link::tor() else {
            log::warn!("Proton's check waits: Tor has not started");
            return false;
        };
        match tor.open_page_door() {
            Ok(door) => Some(door),
            Err(error) => {
                log::error!("no door to Tor for Proton's check: {}", error.kind());
                return false;
            }
        }
    } else {
        None
    };
    let (navigating, leaving) = (report.clone(), report);
    let built = crate::webviews::proton(|builder| through_tor(builder, door)
        .with_url(page.as_str())
        .with_incognito(true)
        .with_initialization_script(RELAY)
        .with_bounds(bounds(app))
        .with_on_page_load_handler(move |event, address| {
            let left = Url::parse(&address).is_ok_and(|url| !is_proton(&url));
            if left && matches!(event, PageLoadEvent::Finished) {
                let leaving = leaving.clone();
                let _ = slint::invoke_from_event_loop(move || leaving(Outcome::Closed));
            }
        })
        .with_navigation_handler(move |address| {
            let Ok(url) = Url::parse(&address) else { return false };
            outcome_of(&url).map_or_else(|| may_load(&url), |outcome| {
                    let navigating = navigating.clone();
                    let _ = slint::invoke_from_event_loop(move || navigating(outcome));
                    false
                })
        })
        .build_as_child(&app.window().window_handle()));
    built.is_ok_and(|view| {
        VIEW.set(Some(view));
        true
    })
}

pub fn place(app: &App) {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.set_bounds(bounds(app));
        }
    });
}

pub fn close() {
    VIEW.set(None);
    if let Some(tor) = crate::tor_link::tor() {
        tor.close_page_door();
    }
}

/// The page by way of Tor's door, with WebRTC unable to go around it. Without a door, as it always was.
fn through_tor(builder: WebViewBuilder<'_>, door: Option<std::net::SocketAddr>) -> WebViewBuilder<'_> {
    let Some(door) = door else { return builder };
    let endpoint = ProxyEndpoint { host: door.ip().to_string(), port: door.port().to_string() };
    let builder = crate::mail_view::without_peer_connections(builder).with_proxy_config(ProxyConfig::Socks5(endpoint));
    windows_without_webrtc(builder, door)
}

/// WebView2 takes its proxy and WebRTC's policy as arguments of its environment. wry writes these defaults itself
/// when none are given, so they are written out here with the two that matter added.
#[cfg(target_os = "windows")]
fn windows_without_webrtc(builder: WebViewBuilder<'_>, door: std::net::SocketAddr) -> WebViewBuilder<'_> {
    use wry::WebViewBuilderExtWindows;
    builder.with_additional_browser_args(format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --proxy-server=socks5://{door} \
         --force-webrtc-ip-handling-policy=disable_non_proxied_udp --webrtc-ip-handling-policy=disable_non_proxied_udp"
    ))
}

#[cfg(not(target_os = "windows"))]
const fn windows_without_webrtc(builder: WebViewBuilder<'_>, _door: std::net::SocketAddr) -> WebViewBuilder<'_> {
    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(url: &str) -> Url {
        Url::parse(url).unwrap_or_else(|_| panic!("{url}"))
    }

    fn outcome(url: &str) -> Option<Outcome> {
        outcome_of(&parsed(url))
    }

    #[test]
    fn a_captcha_needs_no_token_of_its_own() {
        let solved = outcome("proton-mail-light-verify://solved?token=abc&type=captcha");
        assert_eq!(solved, Some(Outcome::Solved(None)));
    }

    #[test]
    fn other_methods_hand_back_their_token() {
        let solved = outcome("proton-mail-light-verify://solved?token=a+b&type=email");
        assert_eq!(solved, Some(Outcome::Solved(Some(("a b".to_string(), "email".to_string())))));
    }

    #[test]
    fn the_page_can_ask_to_leave() {
        assert_eq!(outcome("proton-mail-light-verify://closed"), Some(Outcome::Closed));
    }

    #[test]
    fn ordinary_addresses_are_no_outcome() {
        assert_eq!(outcome("https://verify.proton.me/?token=abc&type=captcha"), None);
    }

    #[test]
    fn opens_only_proton_s_page() {
        assert!(page_url("https://verify.proton.me/?methods=captcha&token=t", "ru", false).is_some());
        assert!(page_url("http://verify.proton.me/", "ru", false).is_none());
        assert!(page_url("https://verify.proton.me.evil.example/", "ru", false).is_none());
        assert!(page_url("not an address", "ru", false).is_none());
    }

    #[test]
    fn keeps_the_challenge_and_adds_language_and_theme() {
        let url = page_url("https://verify.proton.me/?methods=captcha&token=t", "de", true);
        assert_eq!(url.as_ref().and_then(Url::query), Some("methods=captcha&token=t&locale=de&theme=1"));
    }

    #[test]
    fn only_proton_s_own_pages_may_stay_on_screen() {
        let proton = |url: &str| is_proton(&parsed(url));
        assert!(proton("https://verify.proton.me/") && proton("https://proton.me/legal"));
        assert!(!proton("https://proton.me.evil.example/") && !proton("https://evilproton.me/"));
        assert!(!proton("http://verify.proton.me/") && !proton("about:blank"));
    }

    #[test]
    fn loads_the_web_and_nothing_else() {
        let allowed = |url: &str| may_load(&parsed(url));
        assert!(allowed("https://newassets.hcaptcha.com/captcha") && allowed("about:blank"));
        assert!(!allowed("file:///etc/passwd") && !allowed("mailto:a@b.c"));
    }
}
