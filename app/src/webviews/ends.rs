//! A webview whose engine's process ended: ended in Task Manager or Activity Monitor, crashed, or out of memory. Each
//! system says so in its own way, and only this file hears it: it decides what the webview's owner is to do, and hands
//! that over on the window's thread, where the owner makes the page or the webview again with the same content. A page
//! that ends its engine every time is given up after `MOST_TRIES`, so that a hostile mail cannot keep the engine
//! starting and dying: its owner then says so in plain words.
//!
//! The journal gets the owner's fixed word and the kind as a number, never anything the engine wrote.

use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};

/// What the owner of a webview does once the engine behind it has ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remedy {
    /// The page is loaded again in the same webview: the engine starts a new process for it.
    Reload,
    /// The webview is made again: what was behind it is gone.
    #[cfg_attr(
        not(any(target_os = "windows", test)),
        expect(dead_code, reason = "only WebView2 asks for a new webview, though every owner can make one")
    )]
    Remake,
}

/// Who keeps a webview: a fixed word for the journal, and what is done, on the window's thread, once the engine
/// behind it ends.
pub struct Keeper {
    pub what: &'static str,
    pub ended: fn(Remedy),
}

/// Tries after an end that a page gets before it is given up.
pub const MOST_TRIES: u32 = 2;

/// Ends further apart than this are no loop: a system ends a page left in the background now and then to free memory,
/// and so may a person in Task Manager. After a quiet stretch this long the count starts again.
pub const QUIET: Duration = Duration::from_secs(300);

/// How often the engine has ended under one page of an owner, close together.
#[derive(Debug)]
pub struct Tries {
    page: Option<u64>,
    count: u32,
    last: Option<Instant>,
}

impl Tries {
    pub const fn new() -> Self {
        Self { page: None, count: 0, last: None }
    }

    /// The engine ended under `page` at `now`: whether the page may be tried once more. Another page starts the count
    /// again, and so does an end after a quiet stretch.
    pub fn again(&mut self, page: u64, now: Instant) -> bool {
        let quiet = self.last.is_none_or(|last| now.saturating_duration_since(last) >= QUIET);
        if self.page != Some(page) || quiet {
            self.count = 0;
        }
        self.page = Some(page);
        self.last = Some(now);
        self.count = self.count.saturating_add(1);
        self.count <= MOST_TRIES
    }
}

/// The one kind WebKit on macOS reports, as a number for the journal: the page's own process.
pub const WEB_CONTENT: i32 = 0;

/// What WebView2 reported, by its `COREWEBVIEW2_PROCESS_FAILED_KIND`; `None` for a kind it could not say. Microsoft's
/// documentation: the browser process gone (0) closes every webview of it, which must be made again. The main frame's
/// renderer gone (1) leaves an error page, which a reload replaces. An unresponsive renderer (2) is reported again
/// every few seconds while it hangs, and the app chooses when to act: here at once, with a new webview, since a reload
/// may go to the same hung process. A frame's renderer gone (3) leaves that frame broken, and a reload brings it back.
/// Utility (4), sandbox helper (5) and GPU (6) processes recover by themselves, 7 and 8 are no longer used, and an
/// unknown process (9) is not fatal. A kind it could not say is taken as the worst.
#[cfg(any(target_os = "windows", test))]
pub const fn on_windows(kind: Option<i32>) -> Option<Remedy> {
    match kind {
        None | Some(0 | 2) => Some(Remedy::Remake),
        Some(1 | 3) => Some(Remedy::Reload),
        Some(_) => None,
    }
}

/// What WebKitGTK reported, by its `WebKitWebProcessTerminationReason`: crashed (0) or past its memory limit (1), the
/// page is empty until it loads again, which starts a new process. Ended by the app's own call (2), it was on purpose,
/// and the app never makes that call. A reason added later still means the page is gone.
#[cfg(any(target_os = "linux", test))]
pub const fn on_linux(reason: i32) -> Option<Remedy> {
    match reason {
        2 => None,
        _ => Some(Remedy::Reload),
    }
}

thread_local! {
    /// The webviews that exist now, by number. An end reported for one that has gone is late, and is let be.
    static LIVE: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static NUMBERED: Cell<u64> = const { Cell::new(0) };
}

/// A number for the next webview, before it is made: macOS takes the handler on the builder.
pub fn number() -> u64 {
    let number = NUMBERED.get().saturating_add(1);
    NUMBERED.set(number);
    number
}

pub fn alive(number: u64) {
    LIVE.with_borrow_mut(|live| live.push(number));
}

pub fn gone(number: u64) {
    LIVE.with_borrow_mut(|live| live.retain(|other| *other != number));
}

fn is_alive(number: u64) -> bool {
    LIVE.with_borrow(|live| live.contains(&number))
}

/// From the system's callback: noted, and handed to the owner on a later turn of the event loop. Never inside the
/// callback, where the owner could not let go of the webview that is calling.
fn noticed(keeper: &'static Keeper, number: u64, kind: i32, remedy: Option<Remedy>) {
    crate::guard::contain("a web engine's end", || {
        let Some(remedy) = remedy else {
            log::info!("a helper of the engine behind {} ended, kind {kind}: it recovers by itself", keeper.what);
            return;
        };
        log::warn!("the engine behind {} ended, kind {kind}: {remedy:?}", keeper.what);
        let _ = slint::invoke_from_event_loop(move || hand_over(keeper, number, remedy));
    });
}

fn hand_over(keeper: &'static Keeper, number: u64, remedy: Remedy) {
    if is_alive(number) {
        (keeper.ended)(remedy);
    } else {
        log::debug!("an end reported late, for a webview for {} that has gone", keeper.what);
    }
}

/// Given up: the journal says which, once, by its fixed word.
pub fn given_up(keeper: &Keeper) {
    log::warn!("the engine behind {} ended {} times close together: given up", keeper.what, MOST_TRIES + 1);
}

/// Before a webview is made. Only macOS takes the handler on the builder, and reports only the page's own process.
#[cfg(target_os = "macos")]
pub fn before<'a>(builder: wry::WebViewBuilder<'a>, keeper: &'static Keeper, number: u64) -> wry::WebViewBuilder<'a> {
    use wry::WebViewBuilderExtDarwin as _;
    builder.with_on_web_content_process_terminate_handler(move || {
        noticed(keeper, number, WEB_CONTENT, Some(Remedy::Reload));
    })
}

#[cfg(not(target_os = "macos"))]
pub const fn before<'a>(
    builder: wry::WebViewBuilder<'a>,
    _keeper: &'static Keeper,
    _number: u64,
) -> wry::WebViewBuilder<'a> {
    builder
}

/// Once a webview exists, on Windows: WebView2's `ProcessFailed`, on the window's thread that made it.
#[cfg(target_os = "windows")]
pub fn after(view: &wry::WebView, keeper: &'static Keeper, number: u64) {
    use webview2_com::{Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PROCESS_FAILED_KIND, ProcessFailedEventHandler};
    use wry::WebViewExtWindows as _;

    let handler = ProcessFailedEventHandler::create(Box::new(move |_, arguments| {
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
        // Safety: a plain getter on the arguments WebView2 handed this call, which live for it.
        let read = arguments.is_some_and(|arguments| unsafe { arguments.ProcessFailedKind(&raw mut kind) }.is_ok());
        let kind = read.then_some(kind.0);
        noticed(keeper, number, kind.unwrap_or(-1), on_windows(kind));
        Ok(())
    }));
    let mut token = 0_i64;
    // Safety: a call on the webview's own interface, on the window's thread that made it; the handler is kept by it.
    let added = unsafe { view.webview().add_ProcessFailed(&handler, &raw mut token) };
    // The HRESULT alone: a number, never the error's text.
    if let Some(code) = added.err().map(|error| error.code().0) {
        log::warn!("WebView2 will not say when the engine behind {} ends: {code:#010x}", keeper.what);
    }
}

/// Once a webview exists, on Linux: WebKitGTK's `web-process-terminated`.
#[cfg(target_os = "linux")]
pub fn after(view: &wry::WebView, keeper: &'static Keeper, number: u64) {
    use gtk::glib::translate::IntoGlib as _;
    use webkit2gtk::WebViewExt as _;
    use wry::WebViewExtUnix as _;

    view.webview().connect_web_process_terminated(move |_, reason| {
        let reason = reason.into_glib();
        noticed(keeper, number, reason, on_linux(reason));
    });
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub const fn after(_view: &wry::WebView, _keeper: &'static Keeper, _number: u64) {}

#[cfg(test)]
mod tests;
