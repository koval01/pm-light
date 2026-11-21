//! The composer's editor: the page written in, over the window's body. Rich text needs a browser's editing, so this
//! is the one webview with scripts on. Only the script written here can run in it, by the nonce in its policy, and all
//! it can do is hand text back to Rust, which keeps the letter (`DRAFT`). So when the engine behind the page ends, the
//! page is made again from what Rust holds: the letter as the page last reported it, at most a moment's typing behind.

use super::{DRAFT, OPEN, add_paths, on_page_message};
use crate::{
    ComposeWindow, actions,
    webviews::{Held, Keeper, OnePage, Remedy, Tries},
};
use proton_core::{Service, compose::ComposeState, text};
use rand::RngCore;
use slint::ComponentHandle;
use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::Instant,
};
use wry::{DragDropEvent, Rect, http::Request};

const SCRIPT: &str = r#"
const area = document.getElementById("area");
const post = (kind, text) => window.ipc.postMessage(kind + ":" + text);
let timer;
const plain = area.tagName === "TEXTAREA";
const body = () => plain ? area.value : area.innerHTML;
const report = () => { clearTimeout(timer); timer = setTimeout(() => post("body", body()), 150); };
area.addEventListener("input", report);
area.addEventListener("focus", () => post("focus", ""));
if (!plain) area.addEventListener("paste", (event) => {
  event.preventDefault();
  const rich = event.clipboardData.getData("text/html");
  const words = event.clipboardData.getData("text/plain");
  if (rich) post("paste", rich);
  else if (words) document.execCommand("insertText", false, words);
});
document.addEventListener("keydown", (event) => {
  const command = event.metaKey || event.ctrlKey;
  if (event.key === "Escape") post("close", "");
  if (command && event.key === "Enter") { event.preventDefault(); post("send", ""); }
  if (command && event.key === "s") { event.preventDefault(); post("save", ""); }
});
window.format = (command, value) => {
  if (plain) return;
  area.focus();
  document.execCommand(command, false, value);
  post("body", area.innerHTML);
};
window.replaceBody = (text) => { if (plain) area.value = text; else area.innerHTML = text; };
"#;

/// The element that is written in. Plain text takes a `textarea`, whose value is only ever text.
fn writing_area(state: &ComposeState, right_to_left: bool) -> String {
    if state.plain {
        let direction = if right_to_left { r#" dir="rtl""# } else { "" };
        // A parser drops one line break right after the tag, and a letter that starts with room to
        // write would lose a line each time it opened. This is the one it drops.
        format!("<textarea id=\"area\" spellcheck=\"true\"{direction}>\n{}</textarea>", text::escape(&state.body))
    } else {
        format!(r#"<div id="area" contenteditable="true">{}</div>"#, state.body)
    }
}

/// The whole page for a letter.
pub(super) fn page_for(state: &ComposeState, right_to_left: bool, dark: bool) -> String {
    page(&writing_area(state, right_to_left), dark)
}

fn page(area: &str, dark: bool) -> String {
    let mut bytes = [0; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let nonce = proton_core::hex(&bytes);
    let (paper, ink) = if dark { ("#1b1c1f", "#f2f3f5") } else { ("#ffffff", "#17181c") };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="x-dns-prefetch-control" content="off"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-{nonce}'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'"><style>html,body{{height:100%;margin:0;overscroll-behavior:none}}body{{background:{paper};color:{ink};font:14px/1.5 -apple-system,"Segoe UI",system-ui,sans-serif}}#area{{box-sizing:border-box;min-height:100%;padding:14px 20px;outline:none;overflow-wrap:break-word}}#area [style]{{background-color:transparent!important;color:inherit!important}}img{{max-width:100%;height:auto}}blockquote{{margin:8px 0;padding-left:12px;border-left:2px solid #8884}}textarea#area{{display:block;width:100%;height:100%;border:0;resize:none;background:transparent;color:inherit;font:inherit}}</style></head><body>{area}<script nonce="{nonce}">{SCRIPT}</script></body></html>"#
    )
}

pub(super) fn bounds(window: &ComposeWindow) -> Rect {
    crate::webviews::area(
        "the editor",
        f64::from(window.get_body_x()),
        f64::from(window.get_body_y()),
        f64::from(window.get_body_width()),
        f64::from(window.get_body_height()),
    )
}

/// The editor's page, over a protocol: a reply quoting a mail with its pictures can pass the 2 MB
/// WebView2 takes through `with_html`.
static SERVED: OnePage = OnePage::new("composerpage");

static KEEPER: Keeper = Keeper { what: "the editor", ended: engine_ended };

thread_local! {
    static PAGE_NUMBER: Cell<u64> = const { Cell::new(0) };
    /// Numbers the letters opened, so that the engine's ends are counted for one letter at a time.
    static LETTER: Cell<u64> = const { Cell::new(0) };
    static TRIES: RefCell<Tries> = const { RefCell::new(Tries::new()) };
}

fn own_page(url: &str) -> bool {
    SERVED.is_own(url) || url == "about:blank"
}

pub(super) fn script_text(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

pub(super) fn run_in_page(script: &str) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            let _ = open.view.evaluate_script(script);
        }
    });
}

/// Hands `body` to the protocol under a new number, and gives its address.
fn serve(body: &str) -> String {
    let number = PAGE_NUMBER.get() + 1;
    PAGE_NUMBER.set(number);
    SERVED.serve(number, body.to_string())
}

/// The editor for a letter just opened.
pub(super) fn first(service: &Arc<Service>, window: &ComposeWindow, body: &str) -> wry::Result<Held> {
    LETTER.set(LETTER.get() + 1);
    window.set_editor_lost(false);
    make(service, window, body)
}

fn make(service: &Arc<Service>, window: &ComposeWindow, body: &str) -> wry::Result<Held> {
    let (messages, dropped) = (service.clone(), service.clone());
    let address = serve(body);
    crate::webviews::page(&KEEPER, |builder| {
        crate::switches::keep_to_drawing(builder)
            // The page carries its own policy, with the nonce of its one script.
            .with_custom_protocol(SERVED.protocol().to_string(), |_, request| SERVED.answer(&request, None))
            .with_url(&address)
            .with_incognito(true)
            .with_bounds(bounds(window))
            .with_navigation_handler(|url| own_page(&url))
            .with_ipc_handler(move |request: Request<String>| {
                let (service, message) = (messages.clone(), request.into_body());
                let _ = slint::invoke_from_event_loop(move || on_page_message(&service, &message));
            })
            .with_drag_drop_handler(move |event| {
                if let DragDropEvent::Drop { paths, .. } = event {
                    let service = dropped.clone();
                    let _ = slint::invoke_from_event_loop(move || add_paths(&service, paths));
                }
                true
            })
            .build_as_child(&window.window().window_handle())
    })
}

/// The page for the letter as Rust holds it now, which is what the page last reported.
fn page_now(window: &ComposeWindow) -> Option<String> {
    let right_to_left = actions::service().is_some_and(|service| service.habits().right_to_left);
    let dark = window.get_dark();
    DRAFT.lock().as_ref().map(|draft| page_for(&draft.state, right_to_left, dark))
}

/// The engine behind the editor ended: the page comes back, in the same webview or a new one, with the letter as Rust
/// holds it. A letter that ends the engine each time is given up after a few tries: the editor goes out of sight, the
/// window says so, and the letter stays to be saved as a draft or sent.
fn engine_ended(remedy: Remedy) {
    let Some(window) = OPEN.with_borrow(|open| open.as_ref().map(|open| open.window.clone_strong())) else { return };
    if !TRIES.with_borrow_mut(|tries| tries.again(LETTER.get(), Instant::now())) {
        crate::webviews::given_up(&KEEPER);
        give_up(&window);
        return;
    }
    let Some(body) = page_now(&window) else { return };
    match remedy {
        Remedy::Reload => {
            let address = serve(&body);
            OPEN.with_borrow(|open| {
                if let Some(open) = open {
                    let _ = open.view.load_url(&address);
                }
            });
        }
        Remedy::Remake => {
            let Some(service) = actions::service() else { return };
            match make(&service, &window, &body) {
                Ok(view) => replace_view(view),
                Err(error) => {
                    log::error!("no webview for the editor again: {}", crate::webviews::failure(&error));
                    give_up(&window);
                }
            }
        }
    }
}

/// The new webview in the old one's place. The old one goes outside the borrow: on Linux letting a webview go runs GTK's
/// loop, which may call back into the window.
fn replace_view(view: Held) {
    let mut new = Some(view);
    let old = OPEN.with_borrow_mut(|open| {
        let open = open.as_mut()?;
        new.take().map(|new| std::mem::replace(&mut open.view, new))
    });
    drop(old);
    drop(new);
}

fn give_up(window: &ComposeWindow) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            let _ = open.view.set_visible(false);
        }
    });
    window.set_editor_lost(true);
}

#[cfg(test)]
mod tests;
