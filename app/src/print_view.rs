//! Sends a prepared document to the system's print panel. It gets a webview of its own, out of sight,
//! which goes as soon as the panel closes: a mail may be plain text, or several, with no page to print.

use crate::webviews::PageScriptsOff as _;
use crate::{App, mail_view};
use proton_core::print::PrintDocument;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{cell::RefCell, time::Duration};
use wry::{
    PageLoadEvent, Rect, WebView,
    dpi::{LogicalPosition, LogicalSize},
};

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
    static VIEW: RefCell<Option<WebView>> = const { RefCell::new(None) };
    static WATCH: Timer = Timer::default();
}

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

fn go_away() {
    WATCH.with(Timer::stop);
    VIEW.with_borrow_mut(|view| drop(view.take()));
}

#[cfg(target_os = "macos")]
fn panel_is_open(app: &App) -> bool {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_app_kit::NSWindow;

    crate::modal::native_window(app.window()).is_some_and(|window| {
        let window: &NSWindow = &window;
        // Safety: a plain property of a window of this program, read on the main thread.
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

fn place(prepared: &PrintDocument, app: &App) {
    let Some(script) = placing(prepared) else { return };
    let weak = app.as_weak();
    VIEW.with_borrow(|view| {
        if let Some(view) = view {
            let done = view.evaluate_script_with_callback(&script, move |_| {
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || Timer::single_shot(SETTLES, move || ask_system(weak)));
            });
            if done.is_err() {
                log::warn!("a document could not be placed for printing");
            }
        }
    });
}

pub fn print(app: &App, prepared: PrintDocument) {
    go_away();
    let weak = app.as_weak();
    let built = crate::webviews::page(|builder| {
        mail_view::keep_to_drawing(builder)
            .page_scripts_off()
            .with_navigation_handler(|url| own_page(&url))
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, PageLoadEvent::Finished) {
                    let (weak, prepared) = (weak.clone(), prepared.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = weak.upgrade() {
                            place(&prepared, &app);
                        }
                    });
                }
            })
            .with_html(page())
            .with_incognito(true)
            .with_bounds(Rect {
                position: LogicalPosition::new(0.0, 0.0).into(),
                size: LogicalSize::new(1.0, 1.0).into(),
            })
            .with_visible(false)
            .build_as_child(&app.window().window_handle())
    });
    match built {
        Ok(view) => {
            crate::webviews::scripts_settled(&view);
            VIEW.set(Some(view));
        }
        Err(_) => log::warn!("no webview to print with"),
    }
}

#[cfg(test)]
mod tests;
