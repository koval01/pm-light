//! The one place a web engine is used: drawing the body of an HTML mail. It runs no scripts, loads
//! nothing, follows no link, and exists only while such a mail is open.

use crate::App;
use crate::webviews::{Held, Keeper, PageScriptsOff as _, Remedy, Tries};
use layer::{round_the_corner, snapshot};
use slint::{ComponentHandle, Timer, TimerMode};
use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};
use wry::{PageLoadEvent, Rect, WebView};

mod layer;
pub use layer::round_lower_corners;

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
    static VIEW: RefCell<Option<Held>> = const { RefCell::new(None) };
    /// The page goes on screen when it is done, so nobody watches it being drawn.
    static WAITING: Cell<bool> = const { Cell::new(false) };
    /// WebKit is to be started ahead, once the mail's pane has room for it.
    static WARM_LATER: Cell<bool> = const { Cell::new(false) };
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
    /// How often the engine ended under the page now, by its number.
    static TRIES: RefCell<Tries> = const { RefCell::new(Tries::new()) };
}

static KEEPER: Keeper = Keeper { what: "a mail", ended: engine_ended };

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
    SERVED.serve(number, document(number, html, ""))
}

/// The reader's page around a cleaned mail. `hints` are rules of no specificity that go before every other: the
/// legacy attributes Blitz does not read (`html::hints`).
pub fn document(number: u64, html: &str, hints: &str) -> String {
    format!(
        r#"<!doctype html><html><head><title>{number}</title><meta charset="utf-8"><meta http-equiv="x-dns-prefetch-control" content="off"><meta http-equiv="Content-Security-Policy" content="{POLICY}"><style>{hints}</style><style>html{{background:#fff;overscroll-behavior:none}}::-webkit-scrollbar{{width:10px;height:0}}::-webkit-scrollbar-track{{background:transparent}}::-webkit-scrollbar-thumb{{background:#0003;border:2px solid transparent;background-clip:padding-box;border-radius:5px}}body{{margin:0;padding:16px 22px 24px;color:#000;font:14px system-ui,sans-serif;overflow-wrap:break-word;overscroll-behavior:none}}img{{max-width:100%;height:auto}}table{{max-width:100%!important}}td,th{{overflow-wrap:anywhere}}</style></head><body>{html}</body></html>"#
    )
}

/// The toolkit's lengths grow with the zoom, and the system's do not.
fn bounds(app: &App) -> Rect {
    let zoom = crate::look::zoom();
    let length = |value: f32| f64::from(value) * zoom;
    let (x, y) = (length(app.get_body_x()), length(app.get_body_y()));
    crate::webviews::area("a mail", x, y, length(app.get_body_width()), length(app.get_body_height()))
}

/// The engine behind the page ended: the page comes back as it was, from the address it was served at, in the same
/// webview or a new one. A mail that ends the engine each time it loads is given up after a few tries: the webview
/// goes, the pane says in plain words that the mail cannot be shown, and opening it again tries afresh.
fn engine_ended(remedy: Remedy) {
    let Some(app) = crate::actions::main_window() else { return };
    if !TRIES.with_borrow_mut(|tries| tries.again(PAGE.get(), Instant::now())) {
        crate::webviews::given_up(&KEEPER);
        give_up(&app);
        return;
    }
    let Some(address) = SERVED.address() else { return };
    match remedy {
        Remedy::Reload => VIEW.with_borrow(|view| {
            if let Some(view) = view {
                let _ = view.load_url(&address);
            }
        }),
        Remedy::Remake => {
            drop(VIEW.take());
            let made = make(&app, &address, SHOWN.get());
            VIEW.set(made);
        }
    }
}

/// No page in the pane any more, and a plain line in its place, until another mail opens.
fn give_up(app: &App) {
    PATIENCE.with(Timer::stop);
    WAITING.set(false);
    SHOWN.set(false);
    drop(VIEW.take());
    app.set_page_failed(true);
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
    if crate::page_view::chosen() {
        return crate::page_view::show(app, html);
    }
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
    if crate::page_view::chosen() {
        return crate::page_view::show(app, html);
    }
    let address = page(html);
    app.set_page_failed(false);
    let reused = VIEW.with_borrow(|view| {
        view.as_ref()
            .inspect(|existing| {
                round_the_corner(existing, app);
                // The view outlives a change of theme, and its edge shows beside a page that does not fill it.
                let _ = existing.set_background_color(behind_the_page(app));
                let _ = existing.load_url(&address);
            })
            .is_some()
    });
    if reused {
        return;
    }
    let visible = !WAITING.get();
    let made = make(app, &address, visible);
    if made.is_some() {
        SHOWN.set(visible);
    }
    VIEW.set(made);
}

/// A webview for the page at `address`, on screen if `visible` and nothing covers it.
fn make(app: &App, address: &str, visible: bool) -> Option<Held> {
    let built = crate::webviews::page(&KEEPER, |builder| {
        crate::switches::keep_to_drawing(builder)
            .page_scripts_off()
            .with_background_color(behind_the_page(app))
            .with_navigation_handler(|url| own_page(&url))
            // Every link of a mail opens a window of its own (`target="_blank"`, core/src/html.rs), which wry on
            // Linux never shows the navigation handler: the link goes the same way here, and no window opens.
            .with_new_window_req_handler(|url, _| {
                own_page(&url);
                wry::NewWindowResponse::Deny
            })
            .with_on_page_load_handler(|event, _| {
                if matches!(event, PageLoadEvent::Finished) {
                    let _ = slint::invoke_from_event_loop(fit_then_reveal);
                }
            })
            .with_custom_protocol(SERVED.protocol().to_string(), |_, request| SERVED.answer(&request, Some(POLICY)))
            .with_url(address)
            .with_incognito(true)
            .with_visible(visible && COVERS.get() == 0)
            .with_bounds(bounds(app))
            .build_as_child(&app.window().window_handle())
    });
    match built {
        Ok(created) => {
            crate::webviews::scripts_settled(&created);
            round_the_corner(&created, app);
            let _ = created.zoom(crate::look::zoom());
            Some(created)
        }
        Err(error) => {
            log::error!("no webview for mail: {}", crate::webviews::failure(&error));
            crate::actions::toast(crate::toasts::NO_ENGINE);
            None
        }
    }
}

fn fit() {
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.evaluate_script(&fit_script());
        }
    });
}

/// Runs in the page's own world even with scripts off: this is the app speaking, not the mail.
pub fn scroll_page(direction: i32) {
    if crate::page_view::chosen() {
        return crate::page_view::scroll_page(direction);
    }
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
    if crate::page_view::chosen() {
        return crate::page_view::place(app);
    }
    if WARM_LATER.get() && has_room(app) {
        warm_up(app);
    }
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            crate::webviews::place_at(view, bounds(app));
        }
    });
    let width = app.get_body_width();
    // A width in logical pixels: what matters is whether it moved, not by how little.
    if (PLACED_WIDTH.replace(width) - width).abs() > f32::EPSILON {
        fit();
    }
}

/// The preferences chose the other engine. The one let go takes its pages with it: the web view's processes end once
/// Blitz draws, and Blitz's page goes once the web view does. The open mail is drawn again by the new one when the
/// settings close.
pub fn engine_changed() {
    if crate::page_view::chosen() {
        WARM_LATER.set(false);
        set_visible(false);
        drop(VIEW.take());
        crate::page_view::warm_up();
    } else {
        crate::page_view::close();
        if let Some(app) = crate::actions::main_window() {
            warm_up(&app);
        }
    }
}

/// Runs `with` on the mail's page, if there is one. For checks.
pub fn with_page<T>(with: impl FnOnce(&WebView) -> T) -> Option<T> {
    VIEW.with_borrow(|view| view.as_deref().map(with))
}

/// Whether a page is on screen now: made, meant to be seen, drawn whole and with nothing over it. For checks.
pub fn on_screen() -> bool {
    if crate::page_view::chosen() {
        return crate::page_view::on_screen();
    }
    VIEW.with_borrow(Option::is_some) && SHOWN.get() && !WAITING.get() && COVERS.get() == 0
}

/// The webview sits above everything the toolkit draws, so a panel over the mail means hiding it.
pub fn set_visible(visible: bool) {
    if crate::page_view::chosen() {
        return crate::page_view::set_visible(visible);
    }
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
    crate::page_view::close();
    set_visible(false);
    if let Some(app) = crate::actions::main_window() {
        app.set_page_failed(false);
    }
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.load_url(&page(""));
        }
    });
}

/// Nothing of a mail may outlive the session.
pub fn destroy() {
    crate::page_view::close();
    WAITING.set(false);
    drop(VIEW.take());
}

/// Starts WebKit before it is needed, so that the first mail opens like any other. Not before the mail's pane has
/// room: with no mail open it has none, and a page made a pixel wide under Tor, where the mailbox comes late, stayed
/// black on Linux once it grew. It is made as soon as the pane has room.
pub fn warm_up(app: &App) {
    if crate::page_view::chosen() {
        return crate::page_view::warm_up();
    }
    if VIEW.with_borrow(Option::is_some) {
        return;
    }
    if !has_room(app) {
        WARM_LATER.set(true);
        log::debug!("the mail's page waits for room before it is made");
        return;
    }
    WARM_LATER.set(false);
    WAITING.set(true);
    show(app, "");
    WAITING.set(false);
}

/// Whether the mail's pane has room for a page, in the toolkit's lengths.
fn has_room(app: &App) -> bool {
    app.get_body_width() >= ROOM && app.get_body_height() >= ROOM
}

const ROOM: f32 = 32.0;

#[cfg(test)]
mod tests;
