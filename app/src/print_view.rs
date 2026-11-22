//! Sends a prepared document to the system's print panel. It gets a webview of its own, out of sight,
//! which goes as soon as the panel closes: a mail may be plain text, or several, with no page to print.

use crate::App;
use crate::webviews::{Held, Keeper, PageScriptsOff as _, Remedy, Tries};
use proton_core::print::PrintDocument;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{
    cell::{Cell, RefCell},
    time::{Duration, Instant},
};
use wry::PageLoadEvent;

const POLICY: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:";
/// As in the reader: a mail's `:host` lands on an inner element, and nothing of it reaches the marks.
const FRAME_STYLE: &str = ":host{display:block;contain:content;box-sizing:border-box;padding:16px;overflow-x:auto;background:#fff;color:#000;font:14px system-ui,sans-serif;overflow-wrap:break-word}.mail{display:block}";
/// Runs in the app's own world, so it works with the page's scripts off, and each mail stays inside
/// its two shadow roots. Pictures in a mail are decoded by the time this has settled.
const PLACE: &str = "(frame=>doc=>{const root=document.createElement('div');root.id='print-root';const style=document.createElement('style');style.textContent=doc.style;root.append(style);for(const part of doc.parts){const section=document.createElement('section');section.className='print-part';section.innerHTML=part.marks;const host=document.createElement('div');section.append(host);const outer=host.attachShadow({mode:'open'});outer.innerHTML='<style>'+frame+'</style><div class=\"mail\"></div>';const inner=outer.querySelector('.mail').attachShadow({mode:'open'});inner.innerHTML='<style>'+doc.fit+'</style>'+part.html;root.append(section);}document.body.append(root);})";
const SETTLES: Duration = Duration::from_millis(150);
const CHECK_PANEL: Duration = Duration::from_millis(500);
/// Elsewhere there is no asking whether the panel is still open.
#[cfg(not(target_os = "macos"))]
const PANEL_MAY_TAKE: Duration = Duration::from_secs(600);

thread_local! {
    static VIEW: RefCell<Option<Held>> = const { RefCell::new(None) };
    static WATCH: Timer = Timer::default();
    /// The document, until the system's panel has it: what to start again with if the engine ends before then.
    static JOB: RefCell<Option<PrintDocument>> = const { RefCell::new(None) };
    /// Numbers the webviews made, so that what a page that went still reports cannot act on the next one.
    static MADE: Cell<u64> = const { Cell::new(0) };
    /// Numbers the printouts asked for: the engine's ends are counted for one at a time.
    static ASKED: Cell<u64> = const { Cell::new(0) };
    static TRIES: RefCell<Tries> = const { RefCell::new(Tries::new()) };
}

static KEEPER: Keeper = Keeper { what: "printing", ended: engine_ended };

pub fn page() -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="x-dns-prefetch-control" content="off"><meta http-equiv="Content-Security-Policy" content="{POLICY}"><style>html,body{{margin:0;background:#fff}}</style></head><body></body></html>"#
    )
}

/// What the page runs to build the document. The document is JSON, which is also a script's literal.
pub fn placing(prepared: &PrintDocument) -> Option<String> {
    let document = serde_json::to_string(prepared).ok()?;
    let frame = serde_json::to_string(FRAME_STYLE).ok()?;
    Some(format!("{PLACE}({frame})({document});true"))
}

pub fn own_page(url: &str) -> bool {
    url.starts_with("about:") || url.starts_with("data:")
}

pub fn go_away() {
    WATCH.with(Timer::stop);
    drop(JOB.take());
    drop(VIEW.take());
}

#[cfg(target_os = "macos")]
fn panel_is_open(app: &App) -> bool {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::NSWindow;

    crate::modal::native_window(app.window()).is_some_and(|window| {
        let window: &NSWindow = &window;
        // Safety: a plain property of a window of this program, read on the main thread.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let sheet: Option<Retained<AnyObject>> = unsafe { msg_send![window, attachedSheet] };
        sheet.is_some()
    })
}

#[cfg(target_os = "macos")]
fn when_panel_closes(app: &App) {
    let weak = app.as_weak();
    WATCH.with(|timer| {
        timer.start(TimerMode::Repeated, CHECK_PANEL, move || {
            if weak.upgrade().is_none_or(|app| !panel_is_open(&app)) {
                go_away();
            }
        });
    });
}

#[cfg(not(target_os = "macos"))]
fn when_panel_closes(_app: &App) {
    WATCH.with(|timer| timer.start(TimerMode::SingleShot, PANEL_MAY_TAKE, go_away));
}

fn ask_system(weak: slint::Weak<App>) {
    // The panel has the document from here on, and whatever happens to the engine, it is the panel's.
    drop(JOB.take());
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let _ = view.print();
        }
    });
    // The panel is put up from a later turn of the event loop.
    Timer::single_shot(CHECK_PANEL, move || {
        if let Some(app) = weak.upgrade() {
            when_panel_closes(&app);
        }
    });
}

fn place(prepared: &PrintDocument, app: &App, made: u64) {
    let Some(script) = placing(prepared) else { return };
    let weak = app.as_weak();
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let done = view.evaluate_script_with_callback(&script, move |_| {
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    Timer::single_shot(SETTLES, move || {
                        if MADE.get() == made {
                            ask_system(weak);
                        }
                    });
                });
            });
            if done.is_err() {
                log::warn!("a document could not be placed for printing");
            }
        }
    });
}

pub fn print(app: &App, prepared: PrintDocument) {
    if crate::checks::refuses("the system's print panel") {
        return;
    }
    go_away();
    ASKED.set(ASKED.get() + 1);
    make(app, prepared);
}

/// The engine behind the page ended before the system's panel had the document: the printout starts again in a new
/// webview, a few times at most. After the panel has it, the panel holds what it was given, and the person is told
/// that this may not have been the whole document.
fn engine_ended(_remedy: Remedy) {
    let Some(prepared) = JOB.with_borrow(Clone::clone) else {
        if VIEW.with_borrow(Option::is_some) {
            crate::actions::toast(crate::toasts::FAILED);
        }
        return;
    };
    if !TRIES.with_borrow_mut(|tries| tries.again(ASKED.get(), Instant::now())) {
        crate::webviews::given_up(&KEEPER);
        go_away();
        crate::actions::toast(crate::toasts::FAILED);
        return;
    }
    let Some(app) = crate::actions::main_window() else { return };
    drop(VIEW.take());
    make(&app, prepared);
}

fn make(app: &App, prepared: PrintDocument) {
    let made = MADE.get() + 1;
    MADE.set(made);
    JOB.set(Some(prepared.clone()));
    let weak = app.as_weak();
    let built = crate::webviews::page(&KEEPER, |builder| {
        crate::switches::keep_to_drawing(builder)
            .page_scripts_off()
            .with_navigation_handler(|url| own_page(&url))
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, PageLoadEvent::Finished) {
                    let (weak, prepared) = (weak.clone(), prepared.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = weak.upgrade().filter(|_| MADE.get() == made) {
                            place(&prepared, &app, made);
                        }
                    });
                }
            })
            .with_html(page())
            .with_incognito(true)
            .with_bounds(crate::webviews::area("printing", 0.0, 0.0, 1.0, 1.0))
            .with_visible(false)
            .build_as_child(&app.window().window_handle())
    });
    match built {
        Ok(view) => {
            crate::webviews::scripts_settled(&view);
            VIEW.set(Some(view));
        }
        Err(error) => log::warn!("no webview to print with: {}", crate::webviews::failure(&error)),
    }
}

#[cfg(test)]
mod tests;
