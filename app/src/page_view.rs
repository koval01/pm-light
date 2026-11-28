//! The mail's page drawn by Blitz inside the window, when the preferences choose it over the system's web view.
//!
//! A thread of its own holds the page: Blitz lays out and paints there, on the stack `proton_core::page` asks for,
//! and a panic in it ends only that thread. The window shows a band of the page, a screen above and below what is in
//! sight, and moves it as the page scrolls: painting a screen takes tens of milliseconds, far more than a frame, so a
//! new band is asked for before the shown one runs out. Panels drawn over the mail are the toolkit's own, as the page
//! is, and need nothing done.

use crate::{App, actions, look, preferences};
use proton_core::page::{Cursor, Fonts, Page, STACK, Sight, TALLEST_BAND};
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer, Weak};
use std::{
    cell::{Cell, RefCell},
    sync::mpsc,
};

/// How far a press may move and still be a click, in the screen's pixels.
const STILL: f64 = 4.0;

enum Ask {
    Open { number: u64, html: String, sight: Sight, band: Band },
    Resize { number: u64, sight: Sight, band: Band },
    Paint { number: u64, band: Band },
    Hover { x: f64, y: f64 },
    Press { x: f64, y: f64, band: Band },
    Drag { x: f64, y: f64, band: Band },
    Click { x: f64, y: f64 },
    Copy,
    Close,
}

impl Ask {
    /// Asks of which only the last of a run matters.
    const fn kind(&self) -> Option<u8> {
        match self {
            Self::Resize { .. } => Some(0),
            Self::Paint { .. } => Some(1),
            Self::Hover { .. } => Some(2),
            Self::Drag { .. } => Some(3),
            _ => None,
        }
    }

    /// Asks after which nothing earlier matters.
    const fn starts_over(&self) -> bool {
        matches!(self, Self::Open { .. } | Self::Close)
    }
}

/// Rows of the page, in the screen's pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Band {
    top: u32,
    rows: u32,
}

/// What the page in the window is: its number, its pane, its length, how far it is scrolled, and the band shown.
#[derive(Default)]
struct Shown {
    number: u64,
    sight: Option<Sight>,
    length: u32,
    scrolled: u32,
    band: Band,
    /// A band was asked for and has not come yet.
    waiting: bool,
    pressed: Option<(f64, f64)>,
    dragged: bool,
}

thread_local! {
    static WORKER: RefCell<Option<mpsc::Sender<Ask>>> = const { RefCell::new(None) };
    static SHOWN: RefCell<Shown> = RefCell::new(Shown::default());
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// Whether the preferences choose Blitz for HTML mail.
pub fn chosen() -> bool {
    preferences::get().mail_engine == 1
}

/// A count of the screen's pixels from a length of the toolkit's.
fn device(length: f32, scale: f32) -> u32 {
    crate::numbers::pixels(length * scale)
}

/// A length of the toolkit's from a count of the screen's pixels.
fn logical(pixels: u32, scale: f32) -> f32 {
    crate::numbers::float(pixels.min(u32::from(u16::MAX))) / scale.max(f32::EPSILON)
}

fn sight_of(app: &App) -> Sight {
    let scale = app.window().scale_factor();
    Sight {
        width: device(app.get_body_width(), scale).max(1),
        height: device(app.get_body_height(), scale).max(1),
        scale: f64::from(scale) * f64::from(look::mail_text()),
    }
}

/// The band to paint for a page scrolled to `scrolled`: a screen above and below the pane, within the page.
fn band_around(scrolled: u32, pane: u32, length: u32) -> Band {
    let rows = pane.saturating_mul(3).clamp(1, TALLEST_BAND);
    let top = scrolled.saturating_sub(pane).min(length.saturating_sub(rows));
    Band { top, rows: rows.min(length.saturating_sub(top)).max(1) }
}

/// Whether the band no longer holds what is in sight with half a screen to spare, where there is more page.
const fn runs_out(band: Band, scrolled: u32, pane: u32, length: u32) -> bool {
    let (start, end) = (band.top, band.top.saturating_add(band.rows));
    let spare = pane / 2;
    let short_above = start > 0 && scrolled < start.saturating_add(spare);
    let short_below = end < length && scrolled.saturating_add(pane).saturating_add(spare) > end;
    short_above || short_below
}

/// The thread that holds the page, made at the first need and again after a panic ended it.
fn send(ask: Ask) {
    let ask = match WORKER.with_borrow(Clone::clone) {
        Some(worker) => match worker.send(ask) {
            Ok(()) => return,
            Err(mpsc::SendError(ask)) => {
                log::warn!("the mail's page thread had ended, and starts again");
                WORKER.set(None);
                ask
            }
        },
        None => ask,
    };
    start();
    if let Some(worker) = WORKER.with_borrow(Clone::clone) {
        let _ = worker.send(ask);
    }
}

fn start() {
    let (asks, taken) = mpsc::channel();
    let started = std::thread::Builder::new().name("mail page".into()).stack_size(STACK).spawn(move || work(&taken));
    match started {
        Ok(_) => WORKER.set(Some(asks)),
        Err(error) => log::error!("no thread for the mail's page: {}", error.kind()),
    }
}

/// Starts the thread and finds the fonts ahead, so that the first mail opens like any other.
pub fn warm_up() {
    if WORKER.with_borrow(Option::is_none) {
        start();
    }
}

/// What came back from the page's thread, on the window's.
enum Told {
    Drawn { number: u64, length: u32, band: Band, pixels: SharedPixelBuffer<Rgba8Pixel> },
    Refused { number: u64 },
    Cursor(Cursor),
    Link(String),
    Copied(String),
}

fn tell(told: Told) {
    let _ = slint::invoke_from_event_loop(move || heard(told));
}

/// The asks of a batch that still matter, in order: nothing before the last opening or closing, and of each kind that
/// only its last counts for, only that one.
fn latest(batch: Vec<Ask>) -> Vec<Ask> {
    let mut kept = Vec::new();
    let mut seen = [false; 4];
    for ask in batch.into_iter().rev() {
        let starts_over = ask.starts_over();
        let repeated = ask
            .kind()
            .and_then(|kind| seen.get_mut(usize::from(kind)))
            .is_some_and(|seen| std::mem::replace(seen, true));
        if !repeated {
            kept.push(ask);
        }
        if starts_over {
            break;
        }
    }
    kept.reverse();
    kept
}

fn work(asks: &mpsc::Receiver<Ask>) {
    let fonts = Fonts::system();
    let mut page: Option<(u64, Page)> = None;
    let mut cursor = Cursor::Default;
    while let Ok(first) = asks.recv() {
        let mut batch = vec![first];
        batch.extend(asks.try_iter());
        for ask in latest(batch) {
            match ask {
                Ask::Open { number, html, sight, band } => {
                    page = None;
                    match Page::open(&html, &fonts, sight) {
                        Ok(opened) => {
                            log::info!("a mail's page laid out by Blitz, {} pixels high", opened.height());
                            page = Some((number, opened));
                            paint(&mut page, number, band);
                        }
                        Err(refused) => {
                            log::info!("a mail's page refused by Blitz: {refused:?}");
                            tell(Told::Refused { number });
                        }
                    }
                }
                Ask::Resize { number, sight, band } => {
                    if let Some((_, open)) = page.as_mut().filter(|(open, _)| *open == number) {
                        open.resize(sight);
                    }
                    paint(&mut page, number, band);
                }
                Ask::Paint { number, band } => paint(&mut page, number, band),
                Ask::Hover { x, y } => {
                    if let Some((_, open)) = page.as_mut() {
                        let now = open.cursor_at(x, y);
                        if now != cursor {
                            cursor = now;
                            tell(Told::Cursor(now));
                        }
                    }
                }
                Ask::Press { x, y, band } => {
                    if let Some((number, open)) = page.as_mut() {
                        open.press(x, y);
                        let number = *number;
                        paint(&mut page, number, band);
                    }
                }
                Ask::Drag { x, y, band } => {
                    if let Some((number, open)) = page.as_mut()
                        && open.drag(x, y)
                    {
                        let number = *number;
                        paint(&mut page, number, band);
                    }
                }
                Ask::Click { x, y } => {
                    if let Some(link) = page.as_ref().and_then(|(_, open)| open.link_at(x, y)) {
                        tell(Told::Link(link));
                    }
                }
                Ask::Copy => {
                    if let Some(text) = page.as_ref().and_then(|(_, open)| open.selected_text()) {
                        tell(Told::Copied(text));
                    }
                }
                Ask::Close => page = None,
            }
        }
    }
}

/// Paints `band` of the page numbered `number`, if that is the one held, and sends it to the window.
fn paint(page: &mut Option<(u64, Page)>, number: u64, band: Band) {
    let Some((held, open)) = page.as_mut() else { return };
    if *held != number {
        return;
    }
    let length = open.height();
    let rows = band.rows.min(length.saturating_sub(band.top)).max(1);
    let width = open.sight().width.max(1);
    let rgba = open.paint(band.top, rows);
    let pixels = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, width, rows);
    tell(Told::Drawn { number, length, band: Band { top: band.top, rows }, pixels });
}

fn heard(told: Told) {
    let Some(app) = actions::main_window() else { return };
    match told {
        Told::Drawn { number, length, band, pixels } => {
            let current = SHOWN.with_borrow_mut(|shown| {
                if shown.number != number {
                    return false;
                }
                shown.length = length;
                shown.band = band;
                shown.waiting = false;
                true
            });
            if current {
                app.set_page_band(Image::from_rgba8(pixels));
                app.set_page_drawn(true);
                moved(&app);
            }
        }
        Told::Refused { number } => {
            if SHOWN.with_borrow(|shown| shown.number == number) {
                app.set_page_drawn(false);
                show_as_text(&app);
            }
        }
        Told::Cursor(cursor) => app.set_page_cursor(match cursor {
            Cursor::Default => 0,
            Cursor::Pointer => 1,
            Cursor::Text => 2,
        }),
        Told::Link(link) => crate::outside::open_link(&link),
        Told::Copied(text) => app.invoke_copy_text(text.into()),
    }
}

/// A mail Blitz refused, too deep, large or long for it, as plain text, as the cleaner shows one past its limits.
fn show_as_text(app: &App) {
    use slint::{ModelRc, VecModel};
    let Some(message) = crate::view::OPEN.lock().clone() else { return };
    let text = proton_core::text::to_text(&message.safe_body);
    let lines: Vec<crate::BodyLine> = crate::view::lines_of(&text)
        .into_iter()
        .map(|(text, link)| crate::BodyLine { text: text.into(), link: link.into() })
        .collect();
    app.set_plain(true);
    app.set_lines(ModelRc::from(std::rc::Rc::new(VecModel::from(lines))));
}

/// Puts the band where the scroll says, and asks for another when this one runs out.
fn moved(app: &App) {
    let scale = app.window().scale_factor();
    let ask = SHOWN.with_borrow_mut(|shown| {
        let pane = shown.sight.map_or(1, |sight| sight.height);
        shown.scrolled = shown.scrolled.min(shown.length.saturating_sub(pane));
        let top = logical(shown.band.top, scale) - logical(shown.scrolled, scale);
        app.set_page_band_top(top);
        app.set_page_band_height(logical(shown.band.rows, scale));
        app.set_page_length(logical(shown.length, scale));
        app.set_page_scrolled(logical(shown.scrolled, scale));
        if shown.waiting || !runs_out(shown.band, shown.scrolled, pane, shown.length) {
            return None;
        }
        shown.waiting = true;
        Some(Ask::Paint { number: shown.number, band: band_around(shown.scrolled, pane, shown.length) })
    });
    if let Some(ask) = ask {
        send(ask);
    }
}

/// Draws a new mail. It appears when its first band has been painted.
pub fn show(app: &App, html: &str) {
    let number = NEXT.get().wrapping_add(1);
    NEXT.set(number);
    let sight = sight_of(app);
    let hinted = proton_core::html::hints::hints(html);
    let page = crate::mail_view::document(number, &hinted.html, &hinted.style);
    SHOWN.set(Shown { number, sight: Some(sight), ..Shown::default() });
    app.set_page_drawn(false);
    app.set_page_cursor(0);
    let band = Band { top: 0, rows: sight.height.saturating_mul(2).clamp(1, TALLEST_BAND) };
    send(Ask::Open { number, html: page, sight, band });
}

/// The pane moved or changed size, or the scale did: laid out again for a new width.
pub fn place(app: &App) {
    let sight = sight_of(app);
    let ask = SHOWN.with_borrow_mut(|shown| {
        if shown.sight == Some(sight) || shown.sight.is_none() {
            return None;
        }
        shown.sight = Some(sight);
        shown.waiting = true;
        Some(Ask::Resize { number: shown.number, sight, band: band_around(shown.scrolled, sight.height, shown.length) })
    });
    if let Some(ask) = ask {
        send(ask);
    }
}

fn scroll_to(app: &App, scrolled: u32) {
    SHOWN.with_borrow_mut(|shown| shown.scrolled = scrolled);
    moved(app);
}

/// The wheel or the trackpad moved by `dy` of the toolkit's lengths.
fn wheel(app: &App, dy: f32) {
    let scale = app.window().scale_factor();
    let by = device(dy.abs(), scale);
    let now = SHOWN.with_borrow(|shown| shown.scrolled);
    scroll_to(app, if dy > 0.0 { now.saturating_sub(by) } else { now.saturating_add(by) });
}

/// A key scrolls by most of a screen, as it did in the web view.
pub fn scroll_page(direction: i32) {
    let Some(app) = actions::main_window() else { return };
    let (now, pane) = SHOWN.with_borrow(|shown| (shown.scrolled, shown.sight.map_or(0, |sight| sight.height)));
    let by = pane.saturating_mul(17) / 20;
    scroll_to(&app, if direction < 0 { now.saturating_sub(by) } else { now.saturating_add(by) });
}

/// A point of the pane, in the toolkit's lengths, as a point of the page in the screen's pixels.
fn on_page(app: &App, x: f32, y: f32) -> (f64, f64) {
    let scale = app.window().scale_factor();
    let scrolled = SHOWN.with_borrow(|shown| shown.scrolled);
    (f64::from(x * scale), f64::from(y * scale) + f64::from(scrolled))
}

fn pointer(app: &App, what: i32, main_button: bool, x: f32, y: f32) {
    let (x, y) = on_page(app, x, y);
    let band = SHOWN.with_borrow(|shown| shown.band);
    match what {
        0 if main_button => {
            SHOWN.with_borrow_mut(|shown| {
                shown.pressed = Some((x, y));
                shown.dragged = false;
            });
            send(Ask::Press { x, y, band });
        }
        1 if main_button => {
            let (pressed, dragged) = SHOWN.with_borrow_mut(|shown| (shown.pressed.take(), shown.dragged));
            if pressed.is_some() && !dragged {
                send(Ask::Click { x, y });
            }
        }
        2 => {
            let pressed = SHOWN.with_borrow_mut(|shown| {
                let far = shown.pressed.is_some_and(|(px, py)| (px - x).abs() > STILL || (py - y).abs() > STILL);
                shown.dragged |= far;
                shown.pressed.is_some() && shown.dragged
            });
            send(if pressed { Ask::Drag { x, y, band } } else { Ask::Hover { x, y } });
        }
        3 => SHOWN.with_borrow_mut(|shown| shown.pressed = None),
        _ => {}
    }
}

/// Hands the window's events for the page to it.
pub fn wire(app: &App) {
    let weak: Weak<App> = app.as_weak();
    app.on_page_wheel({
        let weak = weak.clone();
        move |_dx, dy| {
            if let Some(app) = weak.upgrade() {
                wheel(&app, dy);
            }
        }
    });
    app.on_page_pointer(move |what, main_button, x, y| {
        if let Some(app) = weak.upgrade() {
            pointer(&app, what, main_button, x, y);
        }
    });
    app.on_page_copy_asked(|| send(Ask::Copy));
}

/// Out of sight, and the page let go: nothing of a mail stays on the thread.
pub fn close() {
    NEXT.set(NEXT.get().wrapping_add(1));
    SHOWN.set(Shown { number: NEXT.get(), ..Shown::default() });
    if let Some(app) = actions::main_window() {
        app.set_page_drawn(false);
        app.set_page_band(Image::default());
    }
    if WORKER.with_borrow(Option::is_some) {
        send(Ask::Close);
    }
}

/// Whether a page is drawn in the window now. For checks.
pub fn on_screen() -> bool {
    actions::main_window().is_some_and(|app| app.get_page_drawn())
}

/// Shown or hidden as the reader asks, without letting the page go.
pub fn set_visible(visible: bool) {
    let Some(app) = actions::main_window() else { return };
    let drawn = SHOWN.with_borrow(|shown| shown.length > 0);
    app.set_page_drawn(visible && drawn);
}

#[cfg(test)]
mod tests;
