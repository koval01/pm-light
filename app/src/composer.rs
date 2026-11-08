//! The compose window. Its fields are the toolkit's, and the body is a page of its own: rich text
//! needs a browser's editing, so this is the one webview with scripts on. Only the script written
//! here can run in it, by the nonce in its policy, and all it can do is hand text back to Rust.

use crate::{
    ComposeWindow, FileRow, Say, actions, mail_view, modal, moment_picker,
    recipients::{self, BCC, CC, People, TO},
    tray, view,
};
use parking_lot::Mutex;
use proton_core::{
    Error, Service,
    compose::{self, ComposeLabels, ComposeMode, ComposeState, SendOptions},
    file_kind, html,
    model::{ContactEmail, OutgoingFile},
    text, timing,
};
use rand::RngCore;
use rfd::FileDialog;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::Arc, time::Duration};
use wry::{
    DragDropEvent, Rect, WebView,
    dpi::{LogicalPosition, LogicalSize},
    http::Request,
};

const AUTOSAVE: Duration = Duration::from_millis(2500);
const WIDTH: f32 = 720.0;
const HEIGHT: f32 = 640.0;

const STATUS_IDLE: i32 = 0;
const STATUS_SAVING: i32 = 1;
const STATUS_SAVED: i32 = 2;
const STATUS_FAILED: i32 = 3;

const FAILED_SEND: i32 = 1;
const FAILED_ADDRESS: i32 = 2;
const FAILED_NO_ONE: i32 = 3;
const FAILED_TIME: i32 = 4;
const FAILED_PAST: i32 = 5;

/// What Proton's own client offers.
const EXPIRY_HOURS: [u64; 5] = [0, 1, 24, 168, 672];
const SECONDS_IN_AN_HOUR: u64 = 3600;

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

/// The message being written, shared with the workers that save and send it.
struct Draft {
    state: ComposeState,
    people: People,
    changed: bool,
}

struct Open {
    window: ComposeWindow,
    view: WebView,
    autosave: Timer,
}

static DRAFT: Mutex<Option<Draft>> = Mutex::new(None);
static CONTACTS: Mutex<Vec<ContactEmail>> = Mutex::new(Vec::new());
/// One save at a time: two racing would create two drafts.
static SAVING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

thread_local! {
    static OPEN: RefCell<Option<Open>> = const { RefCell::new(None) };
}

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

fn page(area: &str, dark: bool) -> String {
    let mut bytes = [0; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let nonce = proton_core::hex(&bytes);
    let (paper, ink) = if dark { ("#1b1c1f", "#f2f3f5") } else { ("#ffffff", "#17181c") };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="x-dns-prefetch-control" content="off"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'nonce-{nonce}'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'"><style>html,body{{height:100%;margin:0;overscroll-behavior:none}}body{{background:{paper};color:{ink};font:14px/1.5 -apple-system,"Segoe UI",system-ui,sans-serif}}#area{{box-sizing:border-box;min-height:100%;padding:14px 20px;outline:none;overflow-wrap:break-word}}#area [style]{{background-color:transparent!important;color:inherit!important}}img{{max-width:100%;height:auto}}blockquote{{margin:8px 0;padding-left:12px;border-left:2px solid #8884}}textarea#area{{display:block;width:100%;height:100%;border:0;resize:none;background:transparent;color:inherit;font:inherit}}</style></head><body>{area}<script nonce="{nonce}">{SCRIPT}</script></body></html>"#
    )
}

fn bounds(window: &ComposeWindow) -> Rect {
    Rect {
        position: LogicalPosition::new(f64::from(window.get_body_x()), f64::from(window.get_body_y())).into(),
        size: LogicalSize::new(f64::from(window.get_body_width()), f64::from(window.get_body_height())).into(),
    }
}

/// The editor's page, over a protocol: a reply quoting a mail with its pictures can pass the 2 MB
/// WebView2 takes through `with_html`.
static SERVED: crate::webviews::OnePage = crate::webviews::OnePage::new("composerpage");

thread_local! {
    static PAGE_NUMBER: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn own_page(url: &str) -> bool {
    SERVED.is_own(url) || url == "about:blank"
}

fn script_text(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

fn run_in_page(script: &str) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            let _ = open.view.evaluate_script(script);
        }
    });
}

fn show_files() {
    let row = |name: &str, mime: &str, size: u64| FileRow {
        name: name.into(),
        size: view::size_text(size).into(),
        kind: file_kind::by_name(name, mime).number(),
        ..FileRow::default()
    };
    let rows: Vec<FileRow> = DRAFT.lock().as_ref().map_or_else(Vec::new, |draft| {
        let existing = draft.state.existing.iter().map(|file| row(&file.name, &file.mime, file.size));
        let added = draft.state.files.iter().map(|file| row(&file.name, &file.mime, file.size));
        existing.chain(added).collect()
    });
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            open.window.set_files(ModelRc::from(Rc::new(VecModel::from(rows))));
        }
    });
}

fn set_status(status: i32) {
    let _ = slint::invoke_from_event_loop(move || {
        OPEN.with_borrow(|open| {
            if let Some(open) = open {
                open.window.set_status(status);
            }
        });
    });
}

fn touched() {
    if let Some(draft) = DRAFT.lock().as_mut() {
        draft.changed = true;
    }
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            open.window.set_status(STATUS_IDLE);
            open.autosave.restart();
        }
    });
}

/// What the person typed since the last save goes to Proton as a draft.
async fn save(service: Arc<Service>) {
    let _one_at_a_time = SAVING.lock().await;
    let Some(snapshot) = DRAFT.lock().as_mut().map(|draft| {
        draft.changed = false;
        draft.state.clone()
    }) else {
        return;
    };
    set_status(STATUS_SAVING);
    let result = service.save_draft(&snapshot).await;
    let status = match result {
        Ok(saved) => {
            let mut guard = DRAFT.lock();
            let Some(draft) = guard.as_mut() else { return };
            draft.state.draft_id = Some(saved.id);
            // These are on the server now, and a later save must not upload them again.
            draft.state.files.retain(|file| !snapshot.files.iter().any(|sent| sent.staged_id == file.staged_id));
            draft.state.existing.extend(saved.attachments);
            let status = if draft.changed { STATUS_IDLE } else { STATUS_SAVED };
            drop(guard);
            status
        }
        Err(error) => {
            log::warn!("a draft was not saved: {}", error.kind());
            STATUS_FAILED
        }
    };
    let _ = slint::invoke_from_event_loop(show_files);
    set_status(status);
}

fn on_page_message(service: &Arc<Service>, message: &str) {
    let Some((kind, text)) = message.split_once(':') else { return };
    match kind {
        "body" => {
            if let Some(draft) = DRAFT.lock().as_mut() {
                draft.state.body = text.to_string();
            }
            touched();
        }
        // Whatever is pasted goes through the same cleaning as incoming mail.
        "paste" => {
            let clean = html::for_editor(&html::protect(text).html);
            run_in_page(&format!("window.format('insertHTML', {})", script_text(&clean)));
        }
        "focus" => OPEN.with_borrow(|open| {
            if let Some(open) = open {
                finish_typing(&open.window);
            }
        }),
        "send" => send(service),
        "save" => drop(tokio::spawn(save(service.clone()))),
        "close" => close(service),
        _ => {}
    }
}

fn add_paths(service: &Arc<Service>, paths: Vec<PathBuf>) {
    for path in paths {
        let Ok(data) = std::fs::read(&path) else { continue };
        let data = service.prepared_file(data);
        let name = path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let mime = mime_guess::from_path(&path).first_or_octet_stream().to_string();
        let size = data.len() as u64;
        let staged_id = service.stage_file(data);
        if let Some(draft) = DRAFT.lock().as_mut() {
            draft.state.files.push(OutgoingFile { name, mime, size, staged_id });
        }
    }
    show_files();
    touched();
}

fn take_fields(window: &ComposeWindow) {
    if let Some(draft) = DRAFT.lock().as_mut() {
        draft.state.subject = window.get_subject().to_string();
    }
    touched();
}

fn typed_text(window: &ComposeWindow, field: usize) -> String {
    let text = match field {
        TO => window.get_to(),
        CC => window.get_cc(),
        _ => window.get_bcc(),
    };
    text.to_string()
}

fn set_typed(window: &ComposeWindow, field: usize, text: &str) {
    match field {
        TO => window.set_to(text.into()),
        CC => window.set_cc(text.into()),
        _ => window.set_bcc(text.into()),
    }
}

/// Draws the addresses as they stand, and keeps the message up with them.
fn show_people(window: &ComposeWindow) {
    let room = window.get_field_room().max(0.0);
    // Everything is taken out first: a field set below runs the window's callbacks at once, and one
    // of them may want the draft.
    let shown = {
        let mut guard = DRAFT.lock();
        let Some(draft) = guard.as_mut() else { return };
        draft.state.to = draft.people.joined(TO);
        draft.state.cc = draft.people.joined(CC);
        draft.state.bcc = draft.people.joined(BCC);
        let shown = (
            recipients::chip_lines(&draft.people, TO, room),
            recipients::chip_lines(&draft.people, CC, room),
            recipients::chip_lines(&draft.people, BCC, room),
            recipients::hint_rows(draft.people.hints()),
            i32::try_from(draft.people.chosen()).unwrap_or(0),
        );
        drop(guard);
        shown
    };
    let (to, cc, bcc, hints, chosen) = shown;
    window.set_to_people(to);
    window.set_cc_people(cc);
    window.set_bcc_people(bcc);
    window.set_hints(hints);
    window.set_chosen_hint(chosen);
    window.set_failed(0);
}

/// Asks whether new addresses can receive mail at all. Without an answer nothing is marked: a failed
/// lookup says nothing about the domain, and Proton has the last word when sending.
fn check_domains(service: &Arc<Service>) {
    let domains = DRAFT.lock().as_mut().map(|draft| draft.people.unchecked_domains()).unwrap_or_default();
    for domain in domains {
        let service = service.clone();
        tokio::spawn(async move {
            if matches!(service.domain_accepts_mail(&domain).await, Ok(false)) {
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(draft) = DRAFT.lock().as_mut() {
                        draft.people.mark_dead(domain);
                    }
                    OPEN.with_borrow(|open| {
                        if let Some(open) = open {
                            show_people(&open.window);
                        }
                    });
                });
            }
        });
    }
}

fn with_people(window: &ComposeWindow, change: impl FnOnce(&mut People)) {
    if let Some(draft) = DRAFT.lock().as_mut() {
        change(&mut draft.people);
    }
    show_people(window);
    if let Some(service) = actions::service() {
        check_domains(&service);
    }
    touched();
}

/// Whatever is still being typed counts as written once the person moves on.
fn finish_typing(window: &ComposeWindow) {
    for field in [TO, CC, BCC] {
        let text = typed_text(window, field);
        if !text.is_empty() {
            set_typed(window, field, "");
            with_people(window, |people| people.left(field, &text));
        }
    }
}

fn finish(notice: i32) {
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(open) = OPEN.take() {
            if let Some(app) = actions::main_window() {
                modal::dismiss(app.window(), open.window.window());
            }
            let _ = open.window.hide();
        }
        *DRAFT.lock() = None;
        view::after_compose(notice);
    });
}

/// What the panels of the window ask for. The failure is the number the window words.
fn chosen_options(window: &ComposeWindow) -> Result<SendOptions, i32> {
    let schedule = window.get_schedule();
    let deliver_at = if schedule.is_empty() {
        None
    } else {
        Some(timing::scheduled_time(&schedule).map_err(|error| if matches!(error, Error::PastTime) { FAILED_PAST } else { FAILED_TIME })?)
    };
    let password = Some(window.get_password().to_string()).filter(|password| !password.is_empty());
    let hours = usize::try_from(window.get_expires()).ok().and_then(|position| EXPIRY_HOURS.get(position)).copied().unwrap_or(0);
    Ok(SendOptions {
        deliver_at,
        password_hint: password.as_ref().map(|_| window.get_password_hint().to_string()),
        password,
        expires_in: (hours > 0).then_some(hours * SECONDS_IN_AN_HOUR),
        read_receipt: Some(window.get_read_receipt()),
        ..Default::default()
    })
}

fn send_time_problem(field: &str) -> Option<&'static str> {
    match timing::scheduled_time(field) {
        Ok(_) => None,
        Err(Error::PastTime) => Some("error.pastTime"),
        Err(_) => Some("error.badTime"),
    }
}

fn pick_schedule(window: &ComposeWindow) {
    let held = window.get_schedule().to_string();
    let request = moment_picker::Request {
        heading: window.global::<Say>().invoke_text("compose.pickTime".into()).to_string(),
        value: if held.is_empty() { timing::tomorrow_morning() } else { held },
        with_time: true,
        earliest: Some(timing::today()),
        check: Some(send_time_problem),
    };
    let weak = window.as_weak();
    moment_picker::open(window, &request, move |field| {
        if let Some(window) = weak.upgrade() {
            window.set_schedule(field.into());
            show_schedule(&window);
        }
    });
}

fn show_schedule(window: &ComposeWindow) {
    let say = window.global::<Say>();
    let text = timing::scheduled_time(&window.get_schedule()).map_or_else(
        |_| String::new(),
        |at| say.invoke_text("compose.scheduledFor".into()).replace("{when}", &timing::moment_text(i64::try_from(at).unwrap_or(0))),
    );
    window.set_schedule_text(text.into());
}

fn send(service: &Arc<Service>) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            finish_typing(&open.window);
        }
    });
    let problem = DRAFT.lock().as_ref().map_or(0, |draft| {
        if draft.people.is_empty() {
            FAILED_NO_ONE
        } else if draft.people.has_bad() {
            FAILED_ADDRESS
        } else {
            0
        }
    });
    if problem != 0 {
        OPEN.with_borrow(|open| {
            if let Some(open) = open {
                open.window.set_failed(problem);
            }
        });
        return;
    }
    let chosen = OPEN.with_borrow(|open| open.as_ref().map(|open| chosen_options(&open.window)));
    let chosen = match chosen {
        Some(Ok(chosen)) => chosen,
        Some(Err(problem)) => {
            OPEN.with_borrow(|open| {
                if let Some(open) = open {
                    open.window.set_failed(problem);
                }
            });
            return;
        }
        None => return,
    };
    let Some(state) = DRAFT.lock().as_ref().map(|draft| draft.state.clone()) else { return };
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            open.autosave.stop();
            open.window.set_sending(true);
            open.window.set_failed(0);
        }
    });
    let service = service.clone();
    tokio::spawn(async move {
        // Waits for a running save: its draft id is the one to send.
        let _settled = SAVING.lock().await;
        let id = DRAFT.lock().as_ref().and_then(|draft| draft.state.draft_id.clone());
        let existing = DRAFT.lock().as_ref().map(|draft| (draft.state.existing.clone(), draft.state.files.clone()));
        let mut state = state;
        state.draft_id = id;
        if let Some((existing, files)) = existing {
            state.existing = existing;
            state.files = files;
        }
        let settings = service.mail_settings().await.ok();
        let delay = settings.as_ref().map_or(0, |settings| settings.delay_send_seconds);
        let scheduled = chosen.deliver_at.is_some();
        let started = std::time::Instant::now();
        let options = SendOptions {
            auto_save_contacts: settings.as_ref().map(|settings| settings.auto_save_contacts),
            delay_seconds: Some(delay),
            ..chosen
        };
        match service.send_composed(&state, options).await {
            Ok(id) if delay > 0 && !scheduled => {
                finish(0);
                view::offer_undo(id, delay);
            }
            Ok(_) => {
                let what = if scheduled { "scheduled" } else { "sent" };
                log::info!("a message was {what} in {} ms", started.elapsed().as_millis());
                finish(if scheduled { crate::toasts::SCHEDULED } else { crate::toasts::SENT });
            }
            Err(error) => {
                log::warn!("a message was not sent: {}", error.kind());
                let _ = slint::invoke_from_event_loop(|| {
                    OPEN.with_borrow(|open| {
                        if let Some(open) = open {
                            open.window.set_sending(false);
                            open.window.set_failed(FAILED_SEND);
                        }
                    });
                });
            }
        }
    });
}

/// Keeps what was written: closing is not throwing away.
fn close(service: &Arc<Service>) {
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            finish_typing(&open.window);
            open.autosave.stop();
        }
    });
    let service = service.clone();
    tokio::spawn(async move {
        let changed = DRAFT.lock().as_ref().is_some_and(|draft| draft.changed);
        if changed {
            save(service).await;
        }
        let saved = DRAFT.lock().as_ref().is_some_and(|draft| draft.state.draft_id.is_some());
        finish(if saved { crate::toasts::DRAFT } else { 0 });
    });
}

/// A saved draft goes only after a second press, asked for inside this window: a system alert would
/// stop the whole program until it is answered.
fn discard(service: &Arc<Service>, confirmed: bool) {
    let id = DRAFT.lock().as_ref().and_then(|draft| draft.state.draft_id.clone());
    if id.is_some() && !confirmed {
        OPEN.with_borrow(|open| {
            if let Some(open) = open {
                open.window.set_confirming(true);
            }
        });
        return;
    }
    let service = service.clone();
    tokio::spawn(async move {
        if let Some(id) = id {
            let _ = service.delete_permanently(&[id], "drafts").await;
        }
        finish(0);
    });
}

pub fn set_from(service: &Service, email: &str) {
    let Some((body, from, plain)) =
        DRAFT.lock().as_ref().map(|draft| (draft.state.body.clone(), draft.state.from.clone(), draft.state.plain))
    else {
        return;
    };
    let footer = actions::main_window().map_or_else(String::new, |app| footer_words(&app));
    let body = if plain {
        service.switch_plain_signature(&body, from.as_deref(), email, &footer)
    } else {
        service.switch_signature(&body, email, &footer)
    };
    if let Some(draft) = DRAFT.lock().as_mut() {
        draft.state.from = Some(email.to_string());
        draft.state.body.clone_from(&body);
    }
    OPEN.with_borrow(|open| {
        if let Some(open) = open {
            open.window.set_from(email.into());
        }
    });
    run_in_page(&format!("window.replaceBody({})", script_text(&body)));
    touched();
}

fn show(service: &Arc<Service>, state: ComposeState) {
    if DRAFT.lock().is_some() {
        OPEN.with_borrow(|open| {
            if let Some(open) = open {
                let _ = open.window.show();
            }
        });
        return;
    }
    let Ok(window) = ComposeWindow::new() else { return };
    crate::look::dress(&window.global::<crate::Theme>());
    let from = state.from.clone().or_else(|| service.addresses().first().map(|address| address.email.clone()));
    window.set_from(from.unwrap_or_default().as_str().into());
    window.set_own_close(cfg!(target_os = "macos"));
    window.set_subject(state.subject.as_str().into());
    window.set_show_copies(!state.cc.is_empty() || !state.bcc.is_empty());
    if let Some(app) = actions::main_window() {
        let (width, height) = if service.habits().wide_composer { (f32::MAX, f32::MAX) } else { (WIDTH, HEIGHT) };
        window.window().set_size(modal::fitted(&app, width, height));
    }
    let people = People::new(&state.to, &state.cc, &state.bcc);
    let begins_at_top = people.is_empty();
    window.set_plain(state.plain);
    let body = page(&writing_area(&state, service.habits().right_to_left), window.get_dark());
    *DRAFT.lock() = Some(Draft { state, people, changed: false });
    show_people(&window);
    let _ = window.show();
    let service = service.clone();
    modal::after_show(&window, move |window| open_page(&service, window, &body, begins_at_top));
}

/// The address fields, which the window draws and Rust keeps.
fn connect_fields(window: &ComposeWindow) {
    let weak = window.as_weak();
    window.on_body_moved(move || {
        if let Some(window) = weak.upgrade() {
            OPEN.with_borrow(|open| {
                if let Some(open) = open {
                    let _ = open.view.set_bounds(bounds(&window));
                }
            });
        }
    });
    let weak = window.as_weak();
    window.on_fields_changed(move || {
        if let Some(window) = weak.upgrade() {
            take_fields(&window);
        }
    });
    let weak = window.as_weak();
    window.on_typed(move |field| {
        let Some(window) = weak.upgrade() else { return };
        let field = usize::try_from(field).unwrap_or(TO);
        let text = typed_text(&window, field);
        let contacts = CONTACTS.lock();
        let rest = DRAFT.lock().as_mut().map(|draft| draft.people.typed(field, &text, &contacts));
        drop(contacts);
        if let Some(rest) = rest.filter(|rest| *rest != text) {
            set_typed(&window, field, &rest);
        }
        show_people(&window);
        if let Some(service) = actions::service() {
            check_domains(&service);
        }
    });
    let weak = window.as_weak();
    window.on_commit(move |field| {
        let Some(window) = weak.upgrade() else { return };
        let field = usize::try_from(field).unwrap_or(TO);
        let text = typed_text(&window, field);
        set_typed(&window, field, "");
        with_people(&window, |people| people.commit(field, &text));
    });
    let weak = window.as_weak();
    window.on_field_left(move |field| {
        let Some(window) = weak.upgrade() else { return };
        let field = usize::try_from(field).unwrap_or(TO);
        let text = typed_text(&window, field);
        if !text.is_empty() {
            set_typed(&window, field, "");
            with_people(&window, |people| people.left(field, &text));
        }
    });
    window.on_field_entered(|_| {});
    let weak = window.as_weak();
    window.on_remove(move |field, index| {
        let Some(window) = weak.upgrade() else { return };
        let (field, index) = (usize::try_from(field).unwrap_or(TO), usize::try_from(index).unwrap_or(usize::MAX));
        with_people(&window, |people| people.remove(field, index));
    });
    let weak = window.as_weak();
    window.on_remove_last(move |field| {
        let Some(window) = weak.upgrade() else { return };
        with_people(&window, |people| people.remove_last(usize::try_from(field).unwrap_or(TO)));
    });
    let weak = window.as_weak();
    window.on_step_hint(move |delta| {
        let Some(window) = weak.upgrade() else { return };
        if let Some(draft) = DRAFT.lock().as_mut() {
            draft.people.step(delta);
        }
        show_people(&window);
    });
    let weak = window.as_weak();
    window.on_pick_hint(move |index| {
        let Some(window) = weak.upgrade() else { return };
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        for field in [TO, CC, BCC] {
            set_typed(&window, field, "");
        }
        with_people(&window, |people| people.pick(index));
    });
    let weak = window.as_weak();
    window.on_room_changed(move || {
        if let Some(window) = weak.upgrade() {
            show_people(&window);
        }
    });
}

/// The editor's toolbar and the files.
fn connect_editing(service: &Arc<Service>, window: &ComposeWindow) {
    window.on_format(|command, value| {
        run_in_page(&format!("window.format({}, {})", script_text(&command), script_text(&value)));
    });
    window.on_link(|url| {
        // The same rule as in the app: a mail may only link to the web or to an address.
        if ["https://", "http://", "mailto:"].iter().any(|scheme| url.starts_with(scheme)) {
            run_in_page(&format!("window.format('createLink', {})", script_text(&url)));
        }
    });
    let attaching = service.clone();
    window.on_attach(move || {
        if let Some(paths) = FileDialog::new().pick_files() {
            add_paths(&attaching, paths);
        }
    });
    window.on_remove_file(|index| {
        let index = usize::try_from(index).unwrap_or(usize::MAX);
        if let Some(draft) = DRAFT.lock().as_mut() {
            let kept = draft.state.existing.len();
            if index < kept {
                draft.state.existing.remove(index);
            } else if index - kept < draft.state.files.len() {
                draft.state.files.remove(index - kept);
            }
        }
        show_files();
        touched();
    });
}

/// Sending, scheduling, discarding and closing.
fn connect_actions(service: &Arc<Service>, window: &ComposeWindow) {
    let sending = service.clone();
    window.on_send(move || send(&sending));
    let scheduling = window.as_weak();
    window.on_schedule_tomorrow(move || {
        if let Some(window) = scheduling.upgrade() {
            window.set_schedule(timing::tomorrow_morning().into());
            show_schedule(&window);
        }
    });
    let scheduling = window.as_weak();
    window.on_schedule_pick(move || {
        if let Some(window) = scheduling.upgrade() {
            pick_schedule(&window);
        }
    });
    let scheduling = window.as_weak();
    window.on_schedule_edited(move || {
        if let Some(window) = scheduling.upgrade() {
            show_schedule(&window);
        }
    });
    let discarding = service.clone();
    window.on_discard(move || discard(&discarding, false));
    let discarding = service.clone();
    window.on_discard_confirmed(move || discard(&discarding, true));
    let closing = service.clone();
    window.on_dismissed(move || close(&closing));
    let closing = service.clone();
    window.window().on_close_requested(move || {
        close(&closing);
        slint::CloseRequestResponse::KeepWindowShown
    });
    let picking: Vec<String> = service.addresses().into_iter().map(|address| address.email).collect();
    let weak = window.as_weak();
    window.on_pick_from(move || {
        if let Some(window) = weak.upgrade() {
            tray::show_from_menu(window.window(), &picking);
        }
    });

}

/// Everything that needs the window to exist for the system, which it does only after `show`.
fn open_page(service: &Arc<Service>, window: ComposeWindow, body: &str, begins_at_top: bool) {
    let messages = service.clone();
    let dropped = service.clone();
    let number = PAGE_NUMBER.get() + 1;
    PAGE_NUMBER.set(number);
    let address = SERVED.serve(number, body.to_string());
    let built = crate::webviews::page(|builder| mail_view::keep_to_drawing(builder)
        // The page carries its own policy, with the nonce of its one script.
        .with_custom_protocol(SERVED.protocol().to_string(), |_, request| SERVED.answer(&request, None))
        .with_url(&address)
        .with_incognito(true)
        .with_bounds(bounds(&window))
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
        .build_as_child(&window.window().window_handle()));
    let Ok(view) = built else {
        *DRAFT.lock() = None;
        let _ = window.hide();
        return;
    };

    connect_fields(&window);
    connect_editing(service, &window);
    connect_actions(service, &window);

    let autosave = Timer::default();
    let saving = service.clone();
    autosave.start(TimerMode::SingleShot, AUTOSAVE, move || {
        if DRAFT.lock().as_ref().is_some_and(|draft| draft.changed) {
            drop(tokio::spawn(save(saving.clone())));
        }
    });
    autosave.stop();
    if let Some(app) = actions::main_window() {
        modal::present(app.window(), window.window());
    }
    check_domains(service);
    if begins_at_top {
        window.invoke_focus_first();
    } else {
        let _ = view.focus();
    }
    OPEN.set(Some(Open { window, view, autosave }));
    show_files();
}

/// A worker died: the letter stays as it is, ready to be sent again.
pub fn after_failure() {
    OPEN.with_borrow(|open| {
        if let Some(open) = open.as_ref().filter(|open| open.window.get_sending()) {
            open.window.set_sending(false);
            open.window.set_failed(FAILED_SEND);
        }
    });
}

/// The session is gone: the letter is not saved, because there is nowhere left to save it.
pub fn abandon() {
    if let Some(open) = OPEN.take() {
        if let Some(app) = actions::main_window() {
            modal::dismiss(app.window(), open.window.window());
        }
        let _ = open.window.hide();
    }
    *DRAFT.lock() = None;
    CONTACTS.lock().clear();
}

/// Hands the open compose window to `then`, or gives `then` back when there is none.
#[cfg(target_os = "macos")]
pub fn with_window<F: FnOnce(&slint::Window)>(then: F) -> Option<F> {
    let window = OPEN.with_borrow(|open| open.as_ref().map(|open| open.window.clone_strong()));
    match window {
        Some(window) => {
            then(window.window());
            None
        }
        None => Some(then),
    }
}

/// A new message to someone chosen from the contacts.
pub fn start_to(service: &Arc<Service>, email: &str) {
    let (service, email) = (service.clone(), email.to_string());
    let proton_footer = actions::main_window().map_or_else(String::new, |app| footer_words(&app));
    tokio::spawn(async move {
        let labels = ComposeLabels {
            date_time: String::new(),
            forwarded: String::new(),
            from: String::new(),
            date: String::new(),
            subject: String::new(),
            to: String::new(),
            proton_footer,
        };
        let Ok(mut state) = service.compose(ComposeMode::New, None, &labels).await else { return };
        state.to = email;
        let _ = slint::invoke_from_event_loop(move || show(&service, state));
    });
}

/// Opens a message that is already written, such as a newsletter's unsubscribe by mail.
pub fn start_prepared(service: &Arc<Service>, state: ComposeState) {
    show(service, if service.new_letters_are_plain() { compose::plain::prepared(state) } else { state });
}

/// Starts writing. A reply or a forward begins from the open message.
pub fn start(service: &Arc<Service>, app: &slint::Weak<crate::App>, mode: ComposeMode, labels: ComposeLabels) {
    let reference = if matches!(mode, ComposeMode::New) { None } else { view::open_message_id() };
    start_from(service, app, mode, reference, labels);
}

/// A message whose sending was taken back, which is a draft again.
pub fn start_draft(service: &Arc<Service>, app: &slint::Weak<crate::App>, id: String, labels: ComposeLabels) {
    start_from(service, app, ComposeMode::Draft, Some(id), labels);
}

fn start_from(service: &Arc<Service>, app: &slint::Weak<crate::App>, mode: ComposeMode, reference: Option<String>, labels: ComposeLabels) {
    log::info!("writing: {mode:?}");
    let forward = matches!(mode, ComposeMode::Forward).then(view::open_message).flatten();
    let (service, app) = (service.clone(), app.clone());
    let knowing = service.clone();
    tokio::spawn(async move {
        if let Ok(contacts) = knowing.contact_emails().await {
            *CONTACTS.lock() = contacts;
        }
    });
    tokio::spawn(async move {
        let Ok(mut state) = service.compose(mode, reference.as_deref(), &labels).await else {
            view::failed_toast(&app);
            return;
        };
        if let Some(message) = forward {
            for file in message.attachments.iter().filter(|file| !file.inline) {
                if let Ok(data) = service.attachment(&message.meta.id, file, &message.address_id).await {
                    let staged_id = service.stage_file(data);
                    state.files.push(OutgoingFile {
                        name: file.name.clone(),
                        mime: file.mime.clone(),
                        size: file.size,
                        staged_id,
                    });
                }
            }
        }
        let _ = slint::invoke_from_event_loop(move || show(&service, state));
    });
}

/// The words a quoted message starts with, and its date as this person reads dates.
pub fn labels(app: &crate::App) -> ComposeLabels {
    let date = view::open_message().map_or(0, |message| i64::try_from(message.meta.date / 1000).unwrap_or(0));
    ComposeLabels {
        date_time: timing::moment_text(date),
        forwarded: app.get_word_forwarded().to_string(),
        from: app.get_word_from().to_string(),
        date: app.get_word_date().to_string(),
        subject: app.get_word_subject().to_string(),
        to: app.get_word_to().to_string(),
        proton_footer: footer_words(app),
    }
}

fn footer_words(app: &crate::App) -> String {
    crate::settings::say(app, "compose.protonFooter")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area_for(body: &str, plain: bool) -> String {
        writing_area(&ComposeState { body: body.into(), plain, ..compose::compose_prepared(&[], "", "") }, false)
    }

    #[test]
    fn a_plain_letter_keeps_the_lines_it_starts_with() {
        // One more than the letter has: the parser takes it away again.
        assert!(area_for("\n\n\n\nwords", true).contains(">\n\n\n\n\nwords</textarea>"));
    }

    #[test]
    fn a_plain_letter_cannot_close_its_field() {
        let area = area_for("</textarea><script>x</script>", true);
        assert_eq!(area.matches("</textarea>").count(), 1);
        assert!(!area.contains("<script>"));
    }

    #[test]
    fn a_rich_letter_stays_in_the_editor() {
        assert!(area_for("<b>words</b>", false).contains(r#"contenteditable="true"><b>words</b></div>"#));
    }
}
