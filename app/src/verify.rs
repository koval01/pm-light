//! Proton forbids framing its verification page, so it gets a webview of its own over the sign-in
//! screen. That webview runs Proton's full page, unchanged: a person passes the check there, and
//! this code only learns the outcome. Protection is never bypassed, and nothing here can reach it.

use crate::webviews::{Held, Keeper, Remedy, Tries};
use crate::{App, Auth};
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    net::SocketAddr,
    rc::Rc,
    time::Instant,
};
use url::Url;
use wry::{PageLoadEvent, ProxyConfig, ProxyEndpoint, Rect, WebViewBuilder};

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

/// What the page was opened for: kept, so that it can be made again when the engine behind it ends.
struct Asked {
    page: Url,
    door: Option<SocketAddr>,
    report: Rc<dyn Fn(Outcome)>,
}

thread_local! {
    static VIEW: RefCell<Option<Held>> = const { RefCell::new(None) };
    static ASKED: RefCell<Option<Asked>> = const { RefCell::new(None) };
    /// Numbers the checks opened, so that the engine's ends are counted for one at a time.
    static OPENED: Cell<u64> = const { Cell::new(0) };
    static TRIES: RefCell<Tries> = const { RefCell::new(Tries::new()) };
}

static KEEPER: Keeper = Keeper { what: "the verification page", ended: engine_ended };

fn bounds(app: &App) -> Rect {
    let auth = app.global::<Auth>();
    crate::webviews::area(
        "the verification page",
        f64::from(auth.get_verify_x()),
        f64::from(auth.get_verify_y()),
        f64::from(auth.get_verify_width()),
        f64::from(auth.get_verify_height()),
    )
}

/// `report` gets the outcome on the window's thread, however the page ended.
pub fn open(app: &App, page: &Url, report: impl Fn(Outcome) + 'static) -> bool {
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
    OPENED.set(OPENED.get() + 1);
    app.global::<Auth>().set_verify_note("".into());
    ASKED.set(Some(Asked { page: page.clone(), door, report: Rc::new(report) }));
    let made = make(app, page, door);
    let opened = made.is_some();
    VIEW.set(made);
    opened
}

fn make(app: &App, page: &Url, door: Option<SocketAddr>) -> Option<Held> {
    let built = crate::webviews::proton(&KEEPER, |builder| {
        through_tor(builder, door)
            .with_url(page.as_str())
            .with_incognito(true)
            .with_initialization_script(RELAY)
            .with_bounds(bounds(app))
            .with_on_page_load_handler(move |event, address| {
                let left = Url::parse(&address).is_ok_and(|url| !is_proton(&url));
                if left && matches!(event, PageLoadEvent::Finished) {
                    deliver(Outcome::Closed);
                }
            })
            .with_navigation_handler(move |address| {
                let Ok(url) = Url::parse(&address) else { return false };
                outcome_of(&url).map_or_else(
                    || may_load(&url),
                    |outcome| {
                        deliver(outcome);
                        false
                    },
                )
            })
            .build_as_child(&app.window().window_handle())
    });
    built.inspect_err(|error| log::warn!("no webview for Proton's check: {}", crate::webviews::failure(error))).ok()
}

/// The outcome to whoever opened the page, on the window's thread. Taken out of `ASKED` first: the report closes the
/// page, which lets go of it.
fn deliver(outcome: Outcome) {
    let _ = slint::invoke_from_event_loop(move || {
        let report = ASKED.with_borrow(|asked| asked.as_ref().map(|asked| asked.report.clone()));
        if let Some(report) = report {
            report(outcome);
        }
    });
}

/// The engine behind Proton's page ended: the page is loaded again from the address Proton gave, in the same webview or
/// a new one, and the bar above it says that what the person did in the check may be lost. A page that ends the engine
/// each time is given up after a few tries, and the screen offers the check in the browser instead.
fn engine_ended(remedy: Remedy) {
    let Some(app) = crate::actions::main_window() else { return };
    let auth = app.global::<Auth>();
    if !TRIES.with_borrow_mut(|tries| tries.again(OPENED.get(), Instant::now())) {
        crate::webviews::given_up(&KEEPER);
        drop(VIEW.take());
        auth.set_verify_failed(true);
        auth.set_verify_note(crate::settings::say(&app, "verify.engineStopped").into());
        return;
    }
    let Some((page, door)) = ASKED.with_borrow(|asked| asked.as_ref().map(|asked| (asked.page.clone(), asked.door)))
    else {
        return;
    };
    auth.set_verify_note(crate::settings::say(&app, "verify.reloaded").into());
    match remedy {
        Remedy::Reload => VIEW.with_borrow(|view| {
            if let Some(view) = view {
                let _ = view.load_url(page.as_str());
            }
        }),
        Remedy::Remake => {
            drop(VIEW.take());
            let made = make(&app, &page, door);
            VIEW.set(made);
        }
    }
}

pub fn place(app: &App) {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            crate::webviews::place_at(view, bounds(app));
        }
    });
}

pub fn close() {
    drop(VIEW.take());
    drop(ASKED.take());
    if let Some(tor) = crate::tor_link::tor() {
        tor.close_page_door();
    }
}

/// The page by way of Tor's door, with WebRTC unable to go around it. Without a door, as it always was.
fn through_tor(builder: WebViewBuilder<'_>, door: Option<std::net::SocketAddr>) -> WebViewBuilder<'_> {
    let Some(door) = door else { return builder };
    let endpoint = ProxyEndpoint { host: door.ip().to_string(), port: door.port().to_string() };
    let builder = crate::switches::without_peer_connections(builder).with_proxy_config(ProxyConfig::Socks5(endpoint));
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
mod tests;
