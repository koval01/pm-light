//! A look at an attachment without saving it. The file is a stranger's, so its bytes decide what is shown (`file_kind`), and it is
//! drawn only inside a web engine's sandboxed process, scripts and the network off, from a protocol
//! of its own that serves this one file. The engine lives as long as the sheet does.

use crate::webviews::PageScriptsOff as _;
use crate::{App, PreviewWindow, Say, actions, look, mail_view, modal};
use proton_core::{
    Service,
    file_kind::{self, Examined, Shown, Warning},
    model::{Attachment, Message},
    text::escape,
};
use slint::{Color, ComponentHandle, Timer};
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    sync::Arc,
    time::Duration,
};
use wry::{
    Rect, WebView,
    dpi::{LogicalPosition, LogicalSize},
    http::{Request, Response, StatusCode, header},
};

const WIDTH: f32 = 860.0;
const HEIGHT: f32 = 640.0;
const LOADING: i32 = 0;
const SHOWN: i32 = 1;
const NOT_SHOWN: i32 = 2;
const PROTOCOL: &str = "attachment";
const PAGE_PATH: &str = "/page";
const FILE_PATH: &str = "/file";
/// The same origin on every system, once each has written its protocol its own way.
#[cfg(not(target_os = "windows"))]
const ORIGIN: &str = "attachment://preview";
#[cfg(target_os = "windows")]
const ORIGIN: &str = "http://attachment.preview";
const POLICY: &str =
    "default-src 'none'; img-src 'self'; media-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
/// Rounds the sheet's lower corners, which the file reaches. Only the two below it show.
const CORNER: f64 = 10.0;

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
    /// Numbers the sheets, so that a file arriving late cannot land in a later one.
    static TURN: Cell<u64> = const { Cell::new(0) };
}

struct Open {
    window: PreviewWindow,
    view: Option<WebView>,
    file: Attachment,
    data: Option<Arc<Vec<u8>>>,
}

/// What the protocol hands out. Nothing else exists on it.
struct Served {
    page: Vec<u8>,
    file: Arc<Vec<u8>>,
    file_type: &'static str,
}

fn css(color: Color) -> String {
    format!("rgb({},{},{})", color.red(), color.green(), color.blue())
}

fn page_of(content: &str, background: &str, text: &str) -> Vec<u8> {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="x-dns-prefetch-control" content="off"><style>html,body{{margin:0;height:100%;background:{background};color:{text}}}body{{display:grid;place-items:center}}img,video{{display:block;max-width:100%;max-height:100vh;object-fit:contain}}audio{{width:min(420px,90%)}}pre{{box-sizing:border-box;width:100%;align-self:start;margin:0;padding:4px 16px 16px;font:13px/1.5 ui-monospace,Menlo,Consolas,monospace;overflow-wrap:anywhere;white-space:pre-wrap}}::-webkit-scrollbar{{width:10px;height:0}}</style></head><body>{content}</body></html>"#
    )
    .into_bytes()
}

/// `None` for a PDF, which the engine opens itself and which has no page around it.
fn content_of(shown: &Shown, data: &[u8]) -> Option<String> {
    let file = FILE_PATH.trim_start_matches('/');
    match shown {
        Shown::Picture(_) => Some(format!(r#"<img alt="" src="{file}">"#)),
        Shown::Video(_) => Some(format!(r#"<video controls preload="metadata" src="{file}"></video>"#)),
        Shown::Audio(_) => Some(format!(r#"<audio controls preload="metadata" src="{file}"></audio>"#)),
        Shown::Text => Some(format!("<pre>{}</pre>", escape(&String::from_utf8_lossy(data)))),
        Shown::Pdf => None,
    }
}

const fn type_of(shown: &Shown) -> &'static str {
    match shown {
        Shown::Picture(mime) | Shown::Video(mime) | Shown::Audio(mime) => mime,
        Shown::Pdf => "application/pdf",
        Shown::Text => "text/plain; charset=utf-8",
    }
}

/// The part of the file asked for, inclusive at both ends. A video cannot be sought without this.
fn byte_range(wanted: Option<&str>, length: usize) -> Option<(usize, usize)> {
    let (from, to) = wanted?.strip_prefix("bytes=")?.split_once('-')?;
    let last = length.checked_sub(1)?;
    let (from, to) = match (from.trim(), to.trim()) {
        ("", tail) => (length.saturating_sub(tail.parse().ok()?), last),
        (from, "") => (from.parse().ok()?, last),
        (from, to) => (from.parse().ok()?, to.parse::<usize>().ok()?.min(last)),
    };
    (from <= to && to <= last).then_some((from, to))
}

fn plain_answer(status: StatusCode) -> Response<Cow<'static, [u8]>> {
    let mut answer = Response::new(Cow::Borrowed(&[][..]));
    *answer.status_mut() = status;
    answer
}

/// Every answer forbids guessing a type, and a page may load this file and nothing else.
fn answer(served: &Served, request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let builder = Response::builder()
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, POLICY);
    let built = match request.uri().path() {
        PAGE_PATH => builder.header(header::CONTENT_TYPE, "text/html; charset=utf-8").body(Cow::Owned(served.page.clone())),
        FILE_PATH => {
            let builder = builder.header(header::CONTENT_TYPE, served.file_type).header(header::ACCEPT_RANGES, "bytes");
            let wanted = request.headers().get(header::RANGE).and_then(|value| value.to_str().ok());
            let length = served.file.len();
            match byte_range(wanted, length).and_then(|(from, to)| Some((from, to, served.file.get(from..=to)?))) {
                Some((from, to, part)) => builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::CONTENT_RANGE, format!("bytes {from}-{to}/{length}"))
                    .body(Cow::Owned(part.to_vec())),
                None if wanted.is_some() => {
                    builder.status(StatusCode::RANGE_NOT_SATISFIABLE).body(Cow::Borrowed(&[][..]))
                }
                None => builder.body(Cow::Owned(served.file.as_ref().clone())),
            }
        }
        _ => return plain_answer(StatusCode::NOT_FOUND),
    };
    built.unwrap_or_else(|_| plain_answer(StatusCode::INTERNAL_SERVER_ERROR))
}

/// The toolkit's lengths grow with the zoom, and the system's do not.
fn bounds(window: &PreviewWindow) -> Rect {
    let zoom = look::zoom();
    Rect {
        position: LogicalPosition::new(f64::from(window.get_page_x()) * zoom, f64::from(window.get_page_y()) * zoom).into(),
        size: LogicalSize::new(f64::from(window.get_page_width()) * zoom, f64::from(window.get_page_height()) * zoom).into(),
    }
}

fn draw(window: &PreviewWindow, shown: &Shown, served: Served) -> Option<WebView> {
    let panel = window.global::<crate::Theme>().get_panel();
    let start = if matches!(shown, Shown::Pdf) { FILE_PATH } else { PAGE_PATH };
    let served = Arc::new(served);
    // Video and sound need the engine's media process, which mail has no use for.
    let moving = matches!(shown, Shown::Video(_) | Shown::Audio(_) | Shown::Pdf);
    let built = crate::webviews::page(|builder| {
        let builder = if moving { builder } else { mail_view::keep_to_drawing(builder) };
        builder
            .page_scripts_off()
            .with_background_color((panel.red(), panel.green(), panel.blue(), panel.alpha()))
            .with_custom_protocol(PROTOCOL.to_string(), move |_, request| answer(&served, &request))
            .with_navigation_handler(|url| url.starts_with(ORIGIN) || url.starts_with("about:"))
            .with_url(format!("{ORIGIN}{start}"))
            .with_incognito(true)
            .with_bounds(bounds(window))
            .build_as_child(&window.window().window_handle())
    });
    match built {
        Ok(view) => {
            crate::webviews::scripts_settled(&view);
            let _ = view.zoom(look::zoom());
            mail_view::round_lower_corners(&view, CORNER);
            Some(view)
        }
        Err(error) => {
            log::error!("no webview for a preview: {}", crate::webviews::failure(&error));
            crate::actions::toast(crate::toasts::NO_ENGINE);
            None
        }
    }
}

fn warning_text(window: &PreviewWindow, examined: &Examined) -> String {
    let key = match examined.warning {
        Some(Warning::Program) => "preview.program",
        Some(Warning::NotWhatItClaims) => "preview.notWhatItClaims",
        None if examined.shown.is_none() => "preview.cannotShow",
        None => return String::new(),
    };
    window.global::<Say>().invoke_text(key.into()).to_string()
}

fn arrived(turn: u64, data: Option<Vec<u8>>) {
    if turn != TURN.get() {
        return;
    }
    let Some((window, file)) = OPEN.with_borrow(|open| open.as_ref().map(|open| (open.window.clone_strong(), open.file.clone())))
    else {
        return;
    };
    let Some(data) = data.map(Arc::new) else {
        log::warn!("a file for a preview did not arrive");
        window.set_state(NOT_SHOWN);
        window.set_problem(window.global::<Say>().invoke_text("reader.previewFailed".into()));
        return;
    };
    let examined = file_kind::examine(&file.name, &file.mime, &data);
    // The type found in the bytes and a size: never the file's name.
    log::info!("a preview of {} KB: shown as {:?}, warning {:?}", data.len() / 1024, examined.shown, examined.warning);
    window.set_problem(warning_text(&window, &examined).into());
    window.set_arrived(true);
    OPEN.with_borrow_mut(|open| {
        if let Some(open) = open {
            open.data = Some(data.clone());
        }
    });
    let Some(shown) = examined.shown else {
        window.set_state(NOT_SHOWN);
        return;
    };
    modal::after_show(&window, move |window| {
        let theme = window.global::<crate::Theme>();
        let (background, text) = (css(theme.get_panel()), css(theme.get_foreground()));
        let page = content_of(&shown, &data).map_or_else(Vec::new, |content| page_of(&content, &background, &text));
        let served = Served { page, file: data.clone(), file_type: type_of(&shown) };
        let view = draw(&window, &shown, served);
        window.set_state(if view.is_some() { SHOWN } else { NOT_SHOWN });
        OPEN.with_borrow_mut(|open| {
            if let Some(open) = open {
                open.view = view;
            }
        });
    });
}

fn place() {
    OPEN.with_borrow(|open| {
        if let Some(Open { window, view: Some(view), .. }) = open {
            let _ = view.set_bounds(bounds(window));
        }
    });
}

fn save() {
    let Some((file, data)) = OPEN.with_borrow(|open| {
        let open = open.as_ref()?;
        Some((open.file.clone(), open.data.clone()?))
    }) else {
        return;
    };
    close();
    if let Some(app) = actions::main_window() {
        actions::write_out(&app, &file, &data);
    }
}

pub fn close() {
    let Some(mut open) = OPEN.take() else { return };
    drop(open.view.take());
    if let Some(app) = actions::main_window() {
        modal::dismiss(app.window(), open.window.window());
    }
    let _ = open.window.hide();
    Timer::single_shot(Duration::ZERO, move || drop(open));
}

pub fn open(service: &Arc<Service>, app: &App, message: &Message, file: Attachment) {
    let Some(turn) = open_window(app, &file) else { return };
    let (service, id, address) = (service.clone(), message.meta.id.clone(), message.address_id.clone());
    tokio::spawn(async move {
        let data = service.attachment(&id, &file, &address).await.ok();
        let _ = slint::invoke_from_event_loop(move || arrived(turn, data));
    });
}

/// A text of the app's own, such as its license, in the same viewer as an attachment.
pub fn show_text(app: &App, name: &str, text: Vec<u8>) {
    let file = Attachment {
        id: String::new(),
        name: name.to_string(),
        size: u64::try_from(text.len()).unwrap_or(u64::MAX),
        mime: "text/plain".to_string(),
        key_packets: String::new(),
        signed: false,
        inline: false,
        content_id: None,
        previewable: true,
    };
    if let Some(turn) = open_window(app, &file) {
        arrived(turn, Some(text));
    }
}

/// The viewer, waiting for its file. `None` when one is open already.
fn open_window(app: &App, file: &Attachment) -> Option<u64> {
    if OPEN.with_borrow(Option::is_some) {
        return None;
    }
    let Ok(window) = PreviewWindow::new() else { return None };
    look::dress(&window.global::<crate::Theme>());
    window.set_name(file.name.as_str().into());
    window.set_state(LOADING);
    window.set_own_close(cfg!(target_os = "macos"));
    window.on_dismissed(close);
    window.on_save(save);
    window.on_moved(place);
    window.window().on_close_requested(|| {
        close();
        slint::CloseRequestResponse::KeepWindowShown
    });
    window.window().set_size(modal::fitted(app, WIDTH, HEIGHT));
    let _ = window.show();
    let parent = app.as_weak();
    modal::after_show(&window, move |window| {
        if let Some(app) = parent.upgrade() {
            modal::present(app.window(), window.window());
        }
        window.invoke_begin();
    });
    OPEN.set(Some(Open { window, view: None, file: file.clone(), data: None }));
    TURN.set(TURN.get() + 1);
    Some(TURN.get())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn served() -> Served {
        Served { page: b"page".to_vec(), file: Arc::new(b"0123456789".to_vec()), file_type: "video/mp4" }
    }

    fn asked(path: &str, range: Option<&str>) -> Response<Cow<'static, [u8]>> {
        let mut request = Request::builder().uri(format!("attachment://preview{path}"));
        if let Some(range) = range {
            request = request.header(header::RANGE, range);
        }
        answer(&served(), &request.body(Vec::new()).unwrap())
    }

    #[test]
    fn text_cannot_become_a_tag() {
        let content = content_of(&Shown::Text, b"<script>alert(1)</script>").unwrap();
        assert!(!content.contains("<script>") && content.contains("&lt;script&gt;"));
    }

    #[test]
    fn a_range_is_read_as_a_browser_writes_it() {
        assert_eq!(byte_range(Some("bytes=0-1"), 10), Some((0, 1)));
        assert_eq!(byte_range(Some("bytes=4-"), 10), Some((4, 9)));
        assert_eq!(byte_range(Some("bytes=-3"), 10), Some((7, 9)));
        assert_eq!(byte_range(Some("bytes=5-99"), 10), Some((5, 9)));
    }

    #[test]
    fn a_range_that_makes_no_sense_is_refused() {
        for wanted in ["bytes=9-2", "bytes=10-", "bytes=x-y", "items=0-1", ""] {
            assert_eq!(byte_range(Some(wanted), 10), None, "{wanted}");
        }
        assert_eq!(byte_range(Some("bytes=0-1"), 0), None);
        assert_eq!(byte_range(None, 10), None);
    }

    #[test]
    fn the_file_comes_whole_or_in_parts_with_the_type_found() {
        let whole = asked(FILE_PATH, None);
        assert_eq!(whole.body().as_ref(), b"0123456789");
        assert_eq!(whole.headers().get(header::CONTENT_TYPE).unwrap(), "video/mp4");
        let part = asked(FILE_PATH, Some("bytes=2-4"));
        assert_eq!(part.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(part.body().as_ref(), b"234");
        assert_eq!(part.headers().get(header::CONTENT_RANGE).unwrap(), "bytes 2-4/10");
        assert_eq!(asked(FILE_PATH, Some("bytes=50-")).status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[test]
    fn nothing_else_exists_on_the_protocol() {
        assert_eq!(asked("/other", None).status(), StatusCode::NOT_FOUND);
        assert_eq!(asked("/", None).status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn every_answer_forbids_guessing_and_loading_elsewhere() {
        for path in [PAGE_PATH, FILE_PATH] {
            let answer = asked(path, None);
            assert_eq!(answer.headers().get(header::X_CONTENT_TYPE_OPTIONS).unwrap(), "nosniff");
            assert!(answer.headers().get(header::CONTENT_SECURITY_POLICY).unwrap().to_str().unwrap().contains("default-src 'none'"));
        }
    }
}
