//! Turns what the core publishes into what the window shows. The core runs on its workers and the
//! window on the main thread, so everything crosses over as plain data.

use crate::{
    App, BodyLine, FileRow, FolderRow, HeaderRow, InfoGroup, InfoItem, InfoWindow, MailRow, actions, composer,
    contacts, mail_view, modal, numbers, palette, reader, search,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures::{StreamExt, stream};
use image::{RgbaImage, imageops};
use parking_lot::Mutex;
use proton_core::{
    Service, file_kind,
    model::{Address, Counts, Folder, Message, MessageMeta},
    service::Stage,
    timing,
    view::{Filters, ViewUpdate},
};
use slint::{
    Color, ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer, TimerMode,
    VecModel, Weak,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{
        Arc, LazyLock, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

/// Long enough for a logo that is already on disk, short enough to go unnoticed.
const LOGO_WAIT: Duration = Duration::from_millis(40);
const TRACKERS_KEPT: usize = 256;
const TINTS: [u32; 7] = [0x0043_77bb, 0x002c_4f7c, 0x002f_8f9d, 0x006b_6fc9, 0x00b5_577f, 0x00b9_8235, 0x003f_8f6b];
/// Past this the list has logos for what anyone will see before scrolling on.
const LOGOS_AT_ONCE: usize = 40;
const STEP_SETTLES: Duration = Duration::from_millis(180);
const LOGO_SEARCH: usize = 64;
/// Senders repeat, so one picture serves all their rows. This is enough for several screens of them.
const PICTURES_KEPT: usize = 256;
/// The envelope's flight to the logo, in `app.slint`, with a frame to spare.
const LANDING: Duration = Duration::from_millis(560);
const LOGOS_TOGETHER: usize = 6;
const ROW_AVATAR: f32 = 34.0;
const READER_AVATAR: f32 = 38.0;
pub const OPAQUE: u32 = 0xff00_0000;
/// A folder when the account keeps folders without colours. Labels always have theirs.
const UNCOLOURED: u32 = 0x009a_a0a6;
const SYSTEM: [&str; 10] =
    ["inbox", "starred", "drafts", "scheduled", "snoozed", "sent", "archive", "spam", "trash", "all"];
/// One turn of the splash, up to where the letter lies sealed on the stack.
const SPLASH_AT_LEAST: Duration = Duration::from_millis(1700);

static STARTED: OnceLock<Instant> = OnceLock::new();

/// A row on the core's side of the line, where nothing of the toolkit may go.
#[derive(Clone)]
struct Row {
    id: String,
    sender: String,
    address: String,
    subject: String,
    date: String,
    group: i32,
    initials: String,
    tint: u32,
    unread: bool,
    starred: bool,
    has_files: bool,
    thread: i32,
    expires: i32,
}

pub fn initials(name: &str) -> String {
    let letters: String = name
        .split(|c: char| c.is_whitespace() || c == '@' || c == '.')
        .filter(|word| !word.is_empty())
        .take(2)
        .filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() { "?".to_string() } else { letters }
}

pub fn tint(name: &str) -> u32 {
    let hash = name.chars().fold(0u32, |hash, c| hash.wrapping_mul(31).wrapping_add(u32::from(c)));
    TINTS.get(hash as usize % TINTS.len()).copied().unwrap_or(TINTS[0])
}

fn seconds(meta: &MessageMeta) -> i64 {
    i64::try_from(meta.date / 1000).unwrap_or(0)
}

fn row(meta: &MessageMeta) -> Row {
    let sender = meta.from.name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| meta.from.email.clone());
    Row {
        id: meta.id.clone(),
        initials: initials(&sender),
        tint: tint(&sender),
        address: meta.from.email.clone(),
        sender,
        // Spaces before a subject push it out of line with the rest, and say nothing.
        subject: meta.subject.trim().to_string(),
        date: timing::list_time(seconds(meta)),
        group: timing::age_group(seconds(meta)),
        unread: meta.unread,
        starred: meta.starred,
        has_files: meta.attachment_count > 0,
        thread: i32::try_from(meta.messages).unwrap_or(i32::MAX),
        expires: days_left(meta.expires),
    }
}

/// Days until Proton deletes a mail by itself, as a row shows them: 0 for less than one, -1 when it stays.
fn days_left(expires: u64) -> i32 {
    match timing::left_until(expires) {
        Some(timing::Left::LessThanADay) => 0,
        Some(timing::Left::Days(days)) => i32::try_from(days.max(1)).unwrap_or(i32::MAX),
        None => -1,
    }
}

fn shown(row: Row, group: i32) -> MailRow {
    MailRow {
        id: row.id.into(),
        sender: row.sender.into(),
        subject: row.subject.into(),
        date: row.date.into(),
        group,
        initials: row.initials.into(),
        tint: Color::from_argb_encoded(OPAQUE | row.tint),
        logo: Image::default(),
        has_logo: false,
        logo_pending: false,
        fresh: false,
        checked: false,
        starred: row.starred,
        age: row.group,
        unread: row.unread,
        selected: false,
        has_files: row.has_files,
        thread: row.thread,
        expires: row.expires,
    }
}

/// Only the first row of each age carries the heading.
fn with_headings(rows: Vec<Row>, mut last: i32) -> Vec<MailRow> {
    rows.into_iter()
        .map(|row| {
            let group = row.group;
            let heading = if group == last { 0 } else { group };
            last = group;
            shown(row, heading)
        })
        .collect()
}

/// A logo as it crosses to the window's thread: images of the toolkit stay on that side.
enum Logo {
    Pixels(Vec<u8>, u32, u32),
    Vector(Vec<u8>),
}

fn logo_of(data_url: &str) -> Option<Logo> {
    let (kind, encoded) = data_url.split_once(";base64,")?;
    let bytes = STANDARD.decode(encoded).ok()?;
    if kind.ends_with("svg+xml") {
        return Some(Logo::Vector(bytes));
    }
    let picture = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = picture.dimensions();
    Some(Logo::Pixels(picture.into_raw(), width, height))
}

/// How much larger a picture is drawn before it shrinks to its pixels. The renderer neither clips to a
/// curve nor smooths what it scales, so the smoothness has to be in the picture.
const OVERSAMPLE: u32 = 4;
const THUMBNAIL_POINTS: f32 = 34.0;
/// As a share of its side.
const THUMBNAIL_CORNER: f32 = 0.18;
const THUMBNAILS_AT_ONCE: usize = 24;
/// Keeps a white logo apart from a white panel, in either theme.
const RING: [u8; 4] = [0, 0, 0, 31];
const RING_WIDTH: f32 = 1.0;

/// A disc holding the logo, drawn large with hard edges and then shrunk: the edge comes out even
/// all the way round, and the ring is part of it, so the two cannot disagree.
fn disc(source: &RgbaImage, side: u32) -> RgbaImage {
    let large = side * OVERSAMPLE;
    let longest = source.width().max(source.height()).max(1);
    let scaled = |length: u32| (length * large / longest).max(1);
    let fitted = crate::look::shrink(source, scaled(source.width()), scaled(source.height()));
    let mut canvas = RgbaImage::from_pixel(large, large, image::Rgba([255, 255, 255, 255]));
    let (x, y) = ((large - fitted.width()) / 2, (large - fitted.height()) / 2);
    imageops::overlay(&mut canvas, &fitted, i64::from(x), i64::from(y));
    let radius = numbers::float(large) / 2.0;
    let ring = RING_WIDTH.mul_add(-numbers::float(OVERSAMPLE), radius);
    let [ring_red, ring_green, ring_blue, ring_alpha] = RING.map(f32::from);
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        let reach = (numbers::float(x) + 0.5 - radius).hypot(numbers::float(y) + 0.5 - radius);
        if reach > radius {
            *pixel = image::Rgba([255, 255, 255, 0]);
        } else if reach > ring {
            let share = ring_alpha / 255.0;
            let blend = |value: u8, over: f32| numbers::channel(over.mul_add(share, f32::from(value) * (1.0 - share)));
            *pixel = image::Rgba([
                blend(pixel.0[0], ring_red),
                blend(pixel.0[1], ring_green),
                blend(pixel.0[2], ring_blue),
                255,
            ]);
        }
    }
    crate::look::shrink(&canvas, side, side)
}

/// A logo made round and shrunk, as pixels: work for a worker thread. The window's thread only wraps the result,
/// since decoding and shrinking there held it for a third of a second and the splash's envelope jerked.
fn picture_pixels(logo: &Logo, side: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let source = match logo {
        Logo::Pixels(pixels, width, height) => RgbaImage::from_raw(*width, *height, pixels.clone())?,
        Logo::Vector(bytes) => {
            let drawn = Image::load_from_svg_data(bytes).ok()?.to_rgba8()?;
            RgbaImage::from_raw(drawn.width(), drawn.height(), drawn.as_bytes().to_vec())?
        }
    };
    let finished = disc(&source, side);
    Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(finished.as_raw(), side, side))
}

/// The pixels of a sender's logo for a row, kept by address: the same sender comes back often.
fn row_pixels(address: &str, logo: &Logo) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let side = pixels_for(ROW_AVATAR);
    let key = format!("{side}/{address}");
    let known = ROW_PIXELS.lock().get(&key).cloned();
    if known.is_some() {
        return known;
    }
    // Made with the lock free: other rows' logos are made at the same time.
    let made = picture_pixels(logo, side)?;
    let mut kept = ROW_PIXELS.lock();
    if kept.len() >= PICTURES_KEPT {
        kept.clear();
    }
    kept.insert(key, made.clone());
    drop(kept);
    Some(made)
}

static ROW_PIXELS: LazyLock<Mutex<HashMap<String, SharedPixelBuffer<Rgba8Pixel>>>> = LazyLock::new(Mutex::default);
/// The window's scale, as the bits of an `f32`, for sizing pictures away from its thread.
static SCALE: AtomicU32 = AtomicU32::new(0x4000_0000);

/// Kept up to date from the window's thread, where the scale can be read.
pub fn remember_scale(app: &App) {
    SCALE.store(app.window().scale_factor().to_bits(), Ordering::Relaxed);
}

fn pixels_for(points: f32) -> u32 {
    numbers::pixels(points * f32::from_bits(SCALE.load(Ordering::Relaxed)))
}

/// A corner of a thumbnail, drawn large with a hard edge and shrunk, as a logo's disc is. Pixels, made off the
/// window's thread.
fn thumbnail_pixels(logo: &Logo, side: u32) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let Logo::Pixels(pixels, width, height) = logo else { return None };
    let source = RgbaImage::from_raw(*width, *height, pixels.clone())?;
    let large = side * OVERSAMPLE;
    let mut canvas = crate::look::shrink(&source, large, large);
    let corner = numbers::float(large) * THUMBNAIL_CORNER;
    let reach = numbers::float(large) - corner;
    for (x, y, pixel) in canvas.enumerate_pixels_mut() {
        let (x, y) = (numbers::float(x) + 0.5, numbers::float(y) + 0.5);
        let across = (corner - x).max(x - reach).max(0.0);
        let down = (corner - y).max(y - reach).max(0.0);
        if across.hypot(down) > corner {
            pixel.0[3] = 0;
        }
    }
    let finished = crate::look::shrink(&canvas, side, side);
    Some(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(finished.as_raw(), side, side))
}

/// One at a time, once the mail is on screen: each may have to be downloaded, and the mail must not
/// wait for any of them. Going to another mail stops the rest.
async fn thumbnails_after(service: &Arc<Service>, app: &Weak<App>, message: &Message, row: &str) {
    let files = message.attachments.iter().filter(|file| !file.inline).take(THUMBNAILS_AT_ONCE);
    for (index, file) in files.enumerate() {
        if CURRENT.lock().as_deref() != Some(row) {
            return;
        }
        let Some(small) = service.thumbnail(&message.meta.id, file, &message.address_id).await else { continue };
        let side = pixels_for(THUMBNAIL_POINTS);
        let made =
            tokio::task::spawn_blocking(move || logo_of(&small).and_then(|logo| thumbnail_pixels(&logo, side))).await;
        let Some(pixels) = made.ok().flatten() else { continue };
        let row = row.to_string();
        let _ = app.upgrade_in_event_loop(move |app| {
            if CURRENT.lock().as_deref() != Some(row.as_str()) {
                return;
            }
            let image = Image::from_rgba8(pixels);
            let rows = app.get_files();
            if let Some(row) = rows.row_data(index) {
                rows.set_row_data(index, FileRow { thumbnail: image, has_thumbnail: true, ..row });
            }
        });
    }
}

/// Where a row should be: searching a long list for each logo would stall the window's thread.
fn row_near(rows: &ModelRc<MailRow>, id: &SharedString, index: usize) -> Option<(usize, MailRow)> {
    let first = index.saturating_sub(LOGO_SEARCH);
    let last = (index + LOGO_SEARCH).min(rows.row_count());
    (first..last).find_map(|at| rows.row_data(at).filter(|row| row.id == *id).map(|row| (at, row)))
}

/// A sender's picture: the logo Proton knows for the domain, or, when the person allowed it, their Gravatar.
async fn picture_of(service: &Service, address: &str) -> Option<String> {
    let logo = service.sender_logo(address, false).await;
    if logo.is_some() || !crate::preferences::get().gravatar {
        return logo;
    }
    service.sender_gravatar(address).await
}

/// The pictures made so far, let go when where they come from changes.
pub fn forget_pictures() {
    ROW_PIXELS.lock().clear();
}

fn logos_for(service: &Arc<Service>, app: &Weak<App>, rows: &[Row], base: usize) {
    let wanted: Vec<(usize, String, String)> = rows
        .iter()
        .take(LOGOS_AT_ONCE)
        .enumerate()
        .map(|(at, row)| (base + at, row.id.clone(), row.address.clone()))
        .collect();
    let (service, app) = (service.clone(), app.clone());
    tokio::spawn(stream::iter(wanted).for_each_concurrent(LOGOS_TOGETHER, move |(index, id, address)| {
        let (service, app) = (service.clone(), app.clone());
        async move {
            let logo = picture_of(&service, &address).await;
            let made = tokio::task::spawn_blocking(move || {
                logo.as_deref().and_then(logo_of).and_then(|logo| row_pixels(&address, &logo))
            })
            .await;
            let pixels = made.ok().flatten();
            let id = SharedString::from(id);
            let _ = app.upgrade_in_event_loop(move |app| {
                let rows = app.get_rows();
                if let Some((at, mut row)) = row_near(&rows, &id, index) {
                    if let Some(pixels) = pixels {
                        row.logo = Image::from_rgba8(pixels);
                        row.has_logo = true;
                    }
                    row.logo_pending = false;
                    rows.set_row_data(at, row);
                }
            });
        }
    }));
}

pub fn publish(service: &Arc<Service>, app: &Weak<App>, update: &ViewUpdate) {
    let whole: Option<Vec<Row>> = update.rows.as_ref().map(|rows| rows.iter().map(row).collect());
    if let Some(rows) = &whole {
        logos_for(service, app, rows, 0);
    }
    let added: Vec<Row> = update.added.iter().map(row).collect();
    let page_starts = update
        .rows
        .as_ref()
        .map_or_else(|| usize::try_from(update.total).unwrap_or(0).saturating_sub(added.len()), Vec::len);
    logos_for(service, app, &added, page_starts);
    let updated = update.updated_at.map(|at| timing::hour_text(i64::try_from(at).unwrap_or(0)));
    let changed: Vec<(String, bool, bool, Vec<String>)> =
        update.changed.iter().map(|meta| (meta.id.clone(), meta.unread, meta.starred, meta.labels.clone())).collect();
    let counts = update.counts.clone();
    let (refreshing, failed, has_more) = (update.refreshing, update.error.is_some(), update.has_more);
    let (page_at, pages) = (i32::try_from(update.page_at).unwrap_or(0), i32::try_from(update.pages).unwrap_or(0));
    let (removed, selected) = (update.removed.clone(), update.selected.clone());
    let fresh: HashSet<String> = update.fresh.iter().cloned().collect();
    let checked: HashSet<String> = update.checked.iter().cloned().collect();
    let loading = update.loading;
    *REFUSED.lock() = update.refused_targets.iter().map(ToString::to_string).collect();
    let can_spam = !update.refused_targets.contains(&"spam");
    let (service, weak) = (service.clone(), app.clone());
    let _ = app.upgrade_in_event_loop(move |app| {
        // A window moved to another display draws at another scale: the next pictures follow it.
        remember_scale(&app);
        app.set_updated(updated.unwrap_or_default().into());
        app.set_failed(failed);
        app.set_loading(refreshing);
        if !loading {
            app.set_paging(false);
        }
        if refreshing {
            app.invoke_pulse();
        }
        if !removed.is_empty() {
            drop_rows(&app, &removed);
            if selected.as_ref().is_none_or(|next| CURRENT.lock().as_ref() != Some(next)) {
                follow(&service, &weak, selected.clone());
            }
        } else if let Some(rows) = whole {
            if !refreshing || !rows.is_empty() {
                reveal(&app);
            }
            LAST_GROUP.set(rows.last().map_or(0, |row| row.group));
            let known: HashMap<SharedString, Image> =
                app.get_rows().iter().filter(|row| row.has_logo).map(|row| (row.id.clone(), row.logo)).collect();
            let current = CURRENT.lock().clone().unwrap_or_default();
            let mut shown = with_headings(rows, 0);
            shown.iter_mut().take(LOGOS_AT_ONCE).for_each(|row| row.logo_pending = true);
            for row in &mut shown {
                row.selected = row.id == current;
                row.fresh = fresh.contains(row.id.as_str());
                if let Some(logo) = known.get(&row.id) {
                    row.logo = logo.clone();
                    row.has_logo = true;
                    row.logo_pending = false;
                }
            }
            app.set_rows(ModelRc::from(Rc::new(VecModel::from(shown))));
            if !fresh.is_empty() {
                app.invoke_light_new_mail();
            }
        }
        app.set_has_more(has_more);
        app.set_list_page(page_at);
        app.set_list_pages(pages);
        app.set_can_spam(can_spam);
        apply_changes(&app, &changed);
        show_checked(&app, &checked);
        if let Some(counts) = &counts {
            show_counts(&app, counts);
        }
        if !added.is_empty() {
            let rows = app.get_rows();
            let Some(model) = rows.as_any().downcast_ref::<VecModel<MailRow>>() else { return };
            let mut last = LAST_GROUP.get();
            for row in added {
                let group = row.group;
                model.push(shown(row, if group == last { 0 } else { group }));
                last = group;
            }
            LAST_GROUP.set(last);
        }
    });
}

/// A row's flag costs little to read, so a whole list can be gone through for one change.
fn show_checked(app: &App, checked: &HashSet<String>) {
    let rows = app.get_rows();
    app.set_checked_count(i32::try_from(checked.len()).unwrap_or(i32::MAX));
    for index in 0..rows.row_count() {
        let Some(mut row) = rows.row_data(index) else { continue };
        let on = checked.contains(row.id.as_str());
        if row.checked != on {
            row.checked = on;
            rows.set_row_data(index, row);
        }
    }
}

fn apply_changes(app: &App, changed: &[(String, bool, bool, Vec<String>)]) {
    let rows = app.get_rows();
    for (id, unread, starred, labels) in changed {
        if CURRENT.lock().as_deref() == Some(id.as_str()) {
            app.set_starred(*starred);
            LABELS.lock().clone_from(labels);
        }
        for index in 0..rows.row_count() {
            let found = rows.row_data(index).filter(|row| row.id == id.as_str());
            if let Some(mut row) = found.filter(|row| row.unread != *unread || row.starred != *starred) {
                row.unread = *unread;
                row.starred = *starred;
                rows.set_row_data(index, row);
            }
        }
    }
}

/// Rows leave one by one, so the list stays where it is, and headings are worked out again.
fn drop_rows(app: &App, removed: &[String]) {
    let rows = app.get_rows();
    let Some(model) = rows.as_any().downcast_ref::<VecModel<MailRow>>() else { return };
    let mut index = 0;
    while index < model.row_count() {
        if model.row_data(index).is_some_and(|row| removed.iter().any(|id| row.id == id.as_str())) {
            model.remove(index);
        } else {
            index += 1;
        }
    }
    let mut last = 0;
    for index in 0..model.row_count() {
        let Some(mut row) = model.row_data(index) else { continue };
        let heading = if row.age == last { 0 } else { row.age };
        last = row.age;
        if row.group != heading {
            row.group = heading;
            model.set_row_data(index, row);
        }
    }
}

/// After the open mail left: the next one if the account says so, otherwise nothing is open.
fn follow(service: &Arc<Service>, app: &Weak<App>, next: Option<String>) {
    if let Some(id) = next {
        open_mail(service, app, id);
    } else {
        *CURRENT.lock() = None;
        mail_view::close();
        if let Some(window) = app.upgrade() {
            window.set_has_mail(false);
        }
    }
}

/// What the heading above the list says is about the folder open, whichever that is.
fn show_folder_unread(app: &App) {
    let open = app.get_folders().iter().find(|folder| folder.active);
    app.set_folder_unread(open.map_or(0, |folder| folder.unread));
}

/// The mail not yet read in the inbox, in the one place it is shown from: the window, the Dock and the tray together.
/// Two ways set it, the folders at the start and the list after it, and the badge once followed only a third.
fn show_inbox_unread(app: &App, unread: u64) {
    app.set_inbox_unread(i32::try_from(unread).unwrap_or(i32::MAX));
    crate::tray::show_unread(app, unread);
}

/// Every count the window shows, and the Dock and the tray with them: a count taken by the list itself, after a mail
/// was read, leaves the background check nothing new to report, and the badge kept the old number.
pub fn show_counts(app: &App, counts: &Counts) {
    let unread = |id: &str| counts.get(id).map_or(0, |count| i32::try_from(count.unread).unwrap_or(i32::MAX));
    show_inbox_unread(app, counts.get("inbox").map_or(0, |count| count.unread));
    let folders = app.get_folders();
    for index in 0..folders.row_count() {
        if let Some(mut folder) = folders.row_data(index) {
            let now = unread(folder.id.as_str());
            if folder.unread != now {
                folder.unread = now;
                folders.set_row_data(index, folder);
            }
        }
    }
    show_folder_unread(app);
}

/// The splash leaves once the envelope has sealed at least once, however fast the mail came.
pub fn reveal(app: &App) {
    let shown = STARTED.get_or_init(Instant::now).elapsed();
    let weak = app.as_weak();
    Timer::single_shot(SPLASH_AT_LEAST.saturating_sub(shown), move || {
        if let Some(app) = weak.upgrade() {
            app.set_ready(true);
            // Making a webview holds the window's thread: after the envelope has landed, not while it flies.
            let landed = app.as_weak();
            Timer::single_shot(LANDING, move || {
                if let Some(app) = landed.upgrade() {
                    crate::splash::let_go(&app);
                    mail_view::warm_up(&app);
                }
            });
        }
    });
}

pub fn loading_mail(app: &Weak<App>) {
    let _ = app.upgrade_in_event_loop(|app| app.set_stage(4));
}

pub fn show_stage(app: &Weak<App>, stage: Stage) {
    STARTED.get_or_init(Instant::now);
    let stage = match stage {
        Stage::Session => 1,
        Stage::Connecting => 2,
        Stage::Keys => 3,
    };
    let _ = app.upgrade_in_event_loop(move |app| app.set_stage(stage));
}

pub fn failed(app: &Weak<App>) {
    let _ = app.upgrade_in_event_loop(|app| {
        app.set_failed(true);
        app.set_loading(false);
        app.set_ready(true);
    });
}

thread_local! {
    static LAST_GROUP: Cell<i32> = const { Cell::new(0) };
    static KEYWORD: RefCell<String> = const { RefCell::new(String::new()) };
}

/// `keyword` is `None` when only the toggles moved.
/// A folder as the journal may name it: Proton's own by their name, the account's own only as such,
/// since their names are the person's.
pub fn folder_for_the_journal(id: &str) -> &str {
    let system =
        id.len() <= 16 && id.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'-' || byte.is_ascii_digit());
    if system { id } else { "a folder of the account's own" }
}

pub fn apply_filters(service: &Arc<Service>, app: &Weak<App>, keyword: Option<String>) {
    let Some(window) = app.upgrade() else { return };
    let keyword = KEYWORD.with_borrow_mut(|known| {
        if let Some(keyword) = keyword {
            *known = keyword;
        }
        known.clone()
    });
    let filters = Filters { keyword, unread: window.get_only_unread(), has_attachments: window.get_only_files() };
    // What was searched for stays out: it can be a name or a subject.
    log::info!(
        "filtering: words {}, unread {}, with files {}",
        !filters.keyword.is_empty(),
        filters.unread,
        filters.has_attachments
    );
    let service = service.clone();
    tokio::spawn(async move { service.view_filter(filters).await });
}

pub fn list_title(app: &Weak<App>, id: &str) {
    let id = SharedString::from(id);
    let _ = app.upgrade_in_event_loop(move |app| {
        app.set_folder_id(id.clone());
        app.set_has_mail(false);
        let folders = app.get_folders();
        for index in 0..folders.row_count() {
            if let Some(mut folder) = folders.row_data(index) {
                folder.active = folder.id == id;
                if folder.active {
                    app.set_folder_title(folder.name.clone());
                }
                folders.set_row_data(index, folder);
            }
        }
        show_folder_unread(&app);
    });
}

fn colour_of(hex: Option<&str>) -> u32 {
    hex.and_then(|hex| u32::from_str_radix(hex.trim_start_matches('#'), 16).ok()).unwrap_or(TINTS[0])
}

pub fn folder_rows(app: &Weak<App>, folders: &[Folder], counts: &Counts, active: &str) {
    let inbox = counts.get("inbox").map_or(0, |count| count.unread);
    *FOLDERS.lock() = folders
        .iter()
        .filter(|folder| folder.id == "inbox" || folder.kind != "system")
        .map(|folder| (folder.id.clone(), folder.name.clone(), folder.kind == "label"))
        .collect();
    let has_labels = folders.iter().any(|folder| folder.kind == "label");
    let colours = crate::actions::service().is_none_or(|service| service.habits().folder_colors);
    let order = |kind: &str| match kind {
        "system" => 0,
        "folder" => 1,
        _ => 2,
    };
    let mut sorted: Vec<&Folder> = folders.iter().collect();
    sorted.sort_by_key(|folder| order(folder.kind));
    let mut last = 0;
    let mut rows: Vec<(String, String, i32, bool, i32, u32)> = sorted
        .into_iter()
        .map(|folder| {
            let group = order(folder.kind);
            let section = if group == last { 0 } else { group };
            last = group;
            let unread = counts.get(&folder.id).map_or(0, |count| count.unread);
            let shown = i32::try_from(unread).unwrap_or(i32::MAX);
            let dot = if colours || folder.kind != "folder" { colour_of(folder.color.as_deref()) } else { UNCOLOURED };
            (folder.id.clone(), folder.name.clone(), shown, folder.id == active, section, dot)
        })
        .collect();
    // An empty group keeps its heading, which holds the button that adds to it.
    for group in [1, 2] {
        if !rows.iter().any(|row| row.4 == group) {
            let place = rows.iter().position(|row| row.4 > group).unwrap_or(rows.len());
            rows.insert(place, (String::new(), String::new(), 0, false, group, 0));
        }
    }
    let _ = app.upgrade_in_event_loop(move |app| {
        show_inbox_unread(&app, inbox);
        app.set_has_labels(has_labels);
        let rows: Vec<FolderRow> = rows
            .into_iter()
            .map(|(id, name, unread, active, section, dot)| FolderRow {
                custom: section > 0 || !SYSTEM.contains(&id.as_str()),
                id: id.into(),
                name: name.into(),
                unread,
                active,
                section,
                dot: Color::from_argb_encoded(OPAQUE | dot),
            })
            .collect();
        app.set_folders(ModelRc::from(Rc::new(VecModel::from(rows))));
        show_folder_unread(&app);
    });
}

fn select(app: &App, id: &str) {
    let rows = app.get_rows();
    for index in 0..rows.row_count() {
        if let Some(mut row) = rows.row_data(index) {
            let selected = row.id == id;
            if row.selected != selected {
                row.selected = selected;
                rows.set_row_data(index, row);
            }
        }
    }
}

pub fn size_text(bytes: u64) -> String {
    let units = crate::actions::main_window().map_or_else(String::new, |app| crate::settings::say(&app, "units.size"));
    crate::settings::rows::size_in(&units, bytes)
}

/// Mail made only of attachments has an empty body, and needs no web engine to show that.
fn has_text(message: &Message) -> bool {
    let source = if message.html { message.safe_body.as_str() } else { message.body.as_str() };
    if !message.html {
        return !source.trim().is_empty();
    }
    source.contains("<img")
        || source.split('<').any(|part| part.split_once('>').is_some_and(|(_, text)| !text.trim().is_empty()))
}

/// Splits a text mail into runs, so that each link can be pressed. Mail is hostile input: only web
/// and mail addresses count as links, and `open_link` checks again.
fn lines_of(body: &str) -> Vec<(String, String)> {
    let mut pieces = Vec::new();
    for line in body.lines() {
        let mut rest = line;
        while let Some(start) = ["https://", "http://"].iter().filter_map(|scheme| rest.find(scheme)).min() {
            let Some((before, from_link)) = rest.split_at_checked(start) else { break };
            let end =
                from_link.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"')).unwrap_or(from_link.len());
            let Some((found, after)) = from_link.split_at_checked(end) else { break };
            let link = found.trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
            let tail = found.strip_prefix(link).unwrap_or_default();
            if !before.trim().is_empty() {
                pieces.push((before.trim_end().to_string(), String::new()));
            }
            pieces.push((link.to_string(), link.to_string()));
            rest = after;
            if !tail.is_empty() {
                pieces.push((tail.to_string(), String::new()));
            }
        }
        pieces.push((rest.to_string(), String::new()));
    }
    pieces
}

fn fill_reader(app: &App, message: &Message, held_back: Option<&str>) {
    let name = message.meta.from.name.clone().filter(|name| !name.is_empty());
    let name = name.unwrap_or_else(|| message.meta.from.email.clone());
    app.set_subject(message.meta.subject.as_str().into());
    app.set_sender_initials(initials(&name).into());
    app.set_sender_tint(Color::from_argb_encoded(OPAQUE | tint(&name)));
    app.set_sender(name.into());
    app.set_address(message.meta.from.email.as_str().into());
    app.set_date(timing::moment_text(seconds(&message.meta)).into());
    let expires = i64::try_from(message.meta.expires).unwrap_or(0);
    let deleted = timing::left_until(message.meta.expires).is_some();
    app.set_expires_at(if deleted { timing::moment_text(expires).into() } else { SharedString::new() });
    let files: Vec<FileRow> = message
        .attachments
        .iter()
        .filter(|file| !file.inline)
        .map(|file| FileRow {
            name: file.name.as_str().into(),
            size: size_text(file.size).into(),
            kind: file_kind::by_name(&file.name, &file.mime).number(),
            thumbnail: Image::default(),
            has_thumbnail: false,
        })
        .collect();
    app.set_files(ModelRc::from(Rc::new(VecModel::from(files))));
    let text = has_text(message);
    app.set_empty_body(!text);
    app.set_plain(!message.html);
    app.set_opening(false);
    if message.html && text {
        mail_view::show_finished(app, held_back.unwrap_or(&message.safe_body));
    } else {
        mail_view::close();
        let lines: Vec<BodyLine> = lines_of(&message.body)
            .into_iter()
            .map(|(text, link)| BodyLine { text: text.into(), link: link.into() })
            .collect();
        app.set_lines(ModelRc::from(Rc::new(VecModel::from(lines))));
    }
    mail_view::place(app);
}

fn threats_of(message: &Message, trackers: &HashMap<String, String>) -> i32 {
    let protection = &message.protection;
    let watching = protection.images.iter().filter(|image| image.pixel || trackers.contains_key(&image.url)).count();
    i32::try_from(watching + protection.links.len() + protection.removed.len()).unwrap_or(i32::MAX)
}

type Item = (i32, String, String);
type Groups = Vec<(i32, Vec<Item>)>;

fn filled(groups: Groups) -> Groups {
    groups.into_iter().filter(|(_, items)| !items.is_empty()).collect()
}

fn host_of(url: &str) -> String {
    let after = url.split_once("://").map_or(url, |(_, rest)| rest);
    after.split(['/', '?', '#']).next().unwrap_or(after).to_string()
}

fn detail_groups(message: &Message) -> Groups {
    let details = &message.details;
    let signature = format!("{:?}", message.signature);
    let mut about = vec![
        (1, String::new(), signature),
        (2, String::new(), details.encryption.clone()),
        (3, String::new(), size_text(message.size)),
    ];
    if !details.message_id.is_empty() {
        about.push((4, String::new(), details.message_id.clone()));
    }
    let checks = details.auth.iter().map(|check| (0, check.method.to_uppercase(), check.result.clone())).collect();
    let people = |list: &[Address]| -> Vec<Item> {
        list.iter()
            .map(|person| {
                person.name.as_ref().map_or_else(
                    || (0, person.email.clone(), String::new()),
                    |name| (0, name.clone(), person.email.clone()),
                )
            })
            .collect()
    };
    filled(vec![(1, about), (10, people(&message.meta.to)), (11, people(&message.cc)), (2, checks)])
}

fn blocked_groups(message: &Message, trackers: &HashMap<String, String>) -> Groups {
    let protection = &message.protection;
    let mut watching: Vec<Item> = Vec::new();
    let mut plain: Vec<Item> = Vec::new();
    for image in &protection.images {
        let host = host_of(&image.url);
        if let Some(name) = trackers.get(&image.url) {
            watching.push((0, name.clone(), host));
        } else if image.pixel {
            watching.push((0, host, String::new()));
        } else if !plain.iter().any(|(_, known, _)| *known == host) {
            plain.push((0, host, String::new()));
        }
    }
    watching.dedup();
    filled(vec![
        (6, watching),
        (7, plain),
        (8, protection.links.iter().map(|link| (0, host_of(&link.to), String::new())).collect()),
        (9, protection.removed.iter().map(|element| (0, element.clone(), String::new())).collect()),
    ])
}

thread_local! {
    static STEPPING: Timer = Timer::default();
    static PENDING: RefCell<Option<String>> = const { RefCell::new(None) };
}

thread_local! {
    static INFO: RefCell<Option<InfoWindow>> = const { RefCell::new(None) };
}

const INFO_WIDTH: f32 = 560.0;
const INFO_HEIGHT: f32 = 620.0;

/// One line of text cut to the width of the window: at a space if there is one near the end.
fn cut_to_width(line: &str, columns: usize) -> Vec<String> {
    let columns = columns.max(1);
    let mut rest: Vec<char> = line.chars().collect();
    let mut pieces = Vec::new();
    while rest.len() > columns {
        let space = rest.iter().take(columns + 1).rposition(|c| *c == ' ').filter(|at| *at > columns / 2);
        let end = space.unwrap_or(columns);
        pieces.push(rest.iter().take(end).collect());
        rest.drain(..space.map_or(end, |at| at + 1));
    }
    pieces.push(rest.into_iter().collect());
    pieces
}

/// Headers as rows of one height, so that scrolling reaches the end whatever the text is. An empty
/// row stands between them, and `true` marks a name.
fn header_rows(headers: &[(String, String)], columns: usize) -> Vec<(String, bool)> {
    let mut rows = Vec::new();
    for (index, (name, text)) in headers.iter().enumerate() {
        if index > 0 {
            rows.push((String::new(), false));
        }
        rows.extend(cut_to_width(name, columns).into_iter().map(|piece| (piece, true)));
        for line in text.lines() {
            rows.extend(cut_to_width(line, columns).into_iter().map(|piece| (piece, false)));
        }
    }
    rows
}

/// The body exactly as it came, with nothing rendered.
fn source_rows(body: &str, columns: usize) -> Vec<(String, bool)> {
    body.lines().flat_map(|line| cut_to_width(line, columns)).map(|piece| (piece, false)).collect()
}

/// A hostile header must not stall the window: this is all a person could read anyway.
const MAX_HEADERS: usize = 400;
const MAX_HEADER_TEXT: usize = 16 * 1024;

/// Headers as name and text. A folded value keeps each of its lines, without the fold's indent.
fn header_lines(raw: &str) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in raw.lines() {
        let named = line.split_once(':').filter(|(name, _)| {
            !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_graphic()) && !line.starts_with([' ', '\t'])
        });
        match (named, headers.last_mut()) {
            (Some((name, text)), _) => headers.push((name.to_string(), text.trim().to_string())),
            (None, Some((_, text))) if !line.trim().is_empty() => {
                text.push('\n');
                text.push_str(line.trim());
            }
            _ => {}
        }
    }
    headers.truncate(MAX_HEADERS);
    for (_, text) in &mut headers {
        if let Some((end, _)) = text.char_indices().nth(MAX_HEADER_TEXT) {
            text.truncate(end);
        }
    }
    headers
}

/// The overlay over the main window. It lives only while it is open, like everything mail leaves behind.
pub fn open_info(mode: i32) {
    if INFO.with_borrow(Option::is_some) {
        return;
    }
    let Some(message) = OPEN.lock().clone() else { return };
    let Some(app) = actions::main_window() else { return };
    let trackers = TRACKERS.lock().clone();
    let groups = match mode {
        2 => blocked_groups(&message, &trackers),
        3 | 4 => Vec::new(),
        _ => detail_groups(&message),
    };
    let groups: Vec<InfoGroup> = groups
        .into_iter()
        .map(|(kind, items)| {
            let items: Vec<InfoItem> = items
                .into_iter()
                .map(|(label, title, detail)| InfoItem { label, title: title.into(), detail: detail.into() })
                .collect();
            InfoGroup { kind, items: ModelRc::from(Rc::new(VecModel::from(items))) }
        })
        .collect();
    let Ok(window) = InfoWindow::new() else { return };
    crate::look::dress(&window.global::<crate::Theme>());
    window.set_mode(mode);
    window.set_subject(message.meta.subject.as_str().into());
    window.set_groups(ModelRc::from(Rc::new(VecModel::from(groups))));
    window.set_raw(if mode == 4 { message.body.as_str() } else { message.raw_headers.as_str() }.into());
    // Windows and Linux keep their own title bar, and with it their own close button.
    window.set_own_close(cfg!(target_os = "macos"));
    window.on_dismissed(close_info);
    if mode == 3 || mode == 4 {
        let headers = header_lines(&message.raw_headers);
        let source = message;
        let weak = window.as_weak();
        window.on_rewrap(move |columns| {
            let Some(window) = weak.upgrade() else { return };
            let columns = usize::try_from(columns).unwrap_or(0);
            let rows = if mode == 3 { header_rows(&headers, columns) } else { source_rows(&source.body, columns) };
            let rows: Vec<HeaderRow> =
                rows.into_iter().map(|(text, name)| HeaderRow { text: text.into(), name }).collect();
            window.set_header_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
        });
    }
    window.window().on_close_requested(|| {
        close_info();
        slint::CloseRequestResponse::KeepWindowShown
    });
    window.window().set_size(modal::fitted(&app, INFO_WIDTH, INFO_HEIGHT));
    let _ = window.show();
    modal::after_show(&window, move |window| {
        window.invoke_rewrap(window.get_columns());
        modal::present(app.window(), window.window());
    });
    INFO.set(Some(window));
}

/// The window the keyboard is with: an overlay if one is open, or else the main one.
#[cfg(target_os = "macos")]
pub fn with_front_window(app: &App, then: impl FnOnce(&slint::Window)) {
    let info = INFO.with_borrow(|info| info.as_ref().map(ComponentHandle::clone_strong));
    let then = match info {
        Some(info) => {
            then(info.window());
            None
        }
        None => Some(then),
    };
    let then = then.and_then(|then| composer::with_window(then));
    let then = then.and_then(|then| contacts::with_window(then));
    let then = then.and_then(|then| crate::print_dialog::with_window(then));
    let then = then.and_then(|then| palette::with_window(then));
    if let Some(then) = then.and_then(|then| search::with_window(then)) {
        then(app.window());
    }
}

fn close_info() {
    let Some(window) = INFO.take() else { return };
    if let Some(app) = actions::main_window() {
        modal::dismiss(app.window(), window.window());
    }
    let _ = window.hide();
    // Not from inside its own callback.
    Timer::single_shot(Duration::ZERO, move || drop(window));
}

/// Nothing of the account stays on screen once its session has ended.
pub fn forget_mailbox(app: &App) {
    close_info();
    search::abandon();
    contacts::abandon();
    palette::abandon();
    mail_view::destroy();
    reader::clear(app);
    *CURRENT.lock() = None;
    *OPEN.lock() = None;
    LABELS.lock().clear();
    FOLDERS.lock().clear();
    KEYWORD.with_borrow_mut(String::clear);
    ROW_PIXELS.lock().clear();
    app.set_has_mail(false);
    app.set_opening(false);
    app.set_rows(ModelRc::default());
    app.set_folders(ModelRc::default());
    app.set_checked_count(0);
    app.set_query_text(SharedString::new());
}

pub fn failed_toast(app: &Weak<App>) {
    let _ = app.upgrade_in_event_loop(|app| app.set_toast(crate::toasts::FAILED));
}

/// The compose window has closed: say what became of the message, and show the list as it is now.
static UNDOABLE: Mutex<Option<String>> = Mutex::new(None);
static KNOWN_TRACKERS: LazyLock<Mutex<HashMap<String, HashMap<String, String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The message went out, and can still be called back for a moment.
pub fn offer_undo(id: String, seconds: u64) {
    *UNDOABLE.lock() = Some(id);
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = actions::main_window() {
            app.set_undo_seconds(i32::try_from(seconds).unwrap_or(0));
        }
    });
}

fn undo_send(service: &Arc<Service>, app: &App) {
    app.set_undo_seconds(0);
    let Some(id) = UNDOABLE.lock().take() else { return };
    let (service, weak, labels) = (service.clone(), app.as_weak(), composer::labels(app));
    tokio::spawn(async move {
        if service.cancel_send(&id).await.is_err() {
            failed_toast(&weak);
            return;
        }
        let (drafting, opening) = (service.clone(), weak.clone());
        let _ = slint::invoke_from_event_loop(move || composer::start_draft(&drafting, &opening, id, labels));
        service.view_reload().await;
    });
}

pub fn connect_undo(service: &Arc<Service>, app: &App) {
    let (service, weak) = (service.clone(), app.as_weak());
    app.on_undo_send(move || {
        if let Some(app) = weak.upgrade() {
            undo_send(&service, &app);
        }
    });
}

pub fn after_compose(notice: i32) {
    actions::after_compose(notice);
}

/// The open message, for what is asked of it later.
static OPEN: Mutex<Option<Arc<Message>>> = Mutex::new(None);
static CURRENT: Mutex<Option<String>> = Mutex::new(None);
static LABELS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static FOLDERS: Mutex<Vec<(String, String, bool)>> = Mutex::new(Vec::new());
static REFUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn current_id() -> Option<String> {
    CURRENT.lock().clone()
}

/// What is being read, which in a conversation is not the row it stands in.
pub fn open_message_id() -> Option<String> {
    OPEN.lock().as_ref().map(|message| message.meta.id.clone())
}

pub fn open_message() -> Option<Arc<Message>> {
    OPEN.lock().clone()
}

pub fn current_sender() -> Option<String> {
    OPEN.lock().as_ref().map(|message| message.meta.from.email.clone())
}

pub fn current_labels() -> Vec<String> {
    LABELS.lock().clone()
}

/// Where the open mail may go, as in `MoveMenu`: the inbox and the account's own folders.
pub fn all_folders() -> Vec<(String, String)> {
    let order = ["starred", "drafts", "scheduled", "snoozed", "sent", "archive", "spam", "trash", "all"];
    let mut list: Vec<(String, String)> = vec![("inbox".to_string(), String::new())];
    list.extend(order.iter().map(|id| ((*id).to_string(), String::new())));
    let custom = FOLDERS.lock();
    list.extend(custom.iter().filter(|(id, ..)| id != "inbox").map(|(id, name, _)| (id.clone(), name.clone())));
    list
}

pub fn move_targets() -> Vec<(String, String)> {
    let refused = REFUSED.lock();
    FOLDERS
        .lock()
        .iter()
        .filter(|(id, _, label)| !label && !refused.contains(id))
        .map(|(id, name, _)| (id.clone(), name.clone()))
        .collect()
}

pub fn labels() -> Vec<(String, String)> {
    FOLDERS.lock().iter().filter(|(_, _, label)| *label).map(|(id, name, _)| (id.clone(), name.clone())).collect()
}
static TRACKERS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);

/// Images come through Proton's proxy and arrive as part of the page, so the webview stays
/// unable to fetch anything itself, and a sender never sees this computer's address.
pub fn show_images(service: &Arc<Service>, app: &Weak<App>) {
    let Some(message) = OPEN.lock().clone() else { return };
    if let Some(window) = app.upgrade() {
        window.set_images_loading(true);
    }
    let (service, app) = (service.clone(), app.clone());
    tokio::spawn(async move {
        let urls = message.protection.images.iter().map(|image| image.url.clone()).collect();
        let html = service.with_remote_images(&message.safe_body, urls).await;
        let _ = app.upgrade_in_event_loop(move |app| {
            app.set_images_held(false);
            app.set_images_loading(false);
            mail_view::show(&app, &html);
        });
    });
}

pub fn close_mail(service: &Arc<Service>, app: &Weak<App>) {
    *CURRENT.lock() = None;
    if let Some(app) = app.upgrade() {
        mail_view::close();
        app.set_has_mail(false);
        select(&app, "");
    }
    let service = service.clone();
    tokio::spawn(async move {
        let _ = service.view_select(None).await;
    });
}

pub fn step(service: &Arc<Service>, app: &Weak<App>, delta: i32) {
    let Some(window) = app.upgrade() else { return };
    let rows = window.get_rows();
    let at = (0..rows.row_count()).find(|index| rows.row_data(*index).is_some_and(|row| row.selected));
    let next = match at {
        Some(at) if delta > 0 => at + 1,
        Some(at) => at.saturating_sub(1),
        None => 0,
    };
    if Some(next) == at {
        return;
    }
    if let Some(row) = rows.row_data(next) {
        window.invoke_pulse();
        step_to(service, app, &window, row.id.to_string());
    }
}

/// Keys come faster than mail opens. The first opens at once, and while they keep coming only the
/// selection moves, so that a run through the list fetches nothing it passes, and marks nothing as
/// read that nobody saw. The mail opens where the run stops.
fn step_to(service: &Arc<Service>, app: &Weak<App>, window: &App, id: String) {
    let running = STEPPING.with(Timer::running);
    if running {
        *CURRENT.lock() = Some(id.clone());
        select(window, &id);
        reader::clear(window);
        window.set_opening(true);
        mail_view::set_visible(false);
        PENDING.set(Some(id));
        STEPPING.with(Timer::restart);
        return;
    }
    open_mail(service, app, id);
    let (service, app) = (service.clone(), app.clone());
    STEPPING.with(|timer| {
        timer.start(TimerMode::SingleShot, STEP_SETTLES, move || {
            if let Some(id) = PENDING.take() {
                open_mail(&service, &app, id);
            }
        });
    });
}

/// Opening a message reads it, as in any mail client, and Proton is told so.
pub fn open_mail(service: &Arc<Service>, app: &Weak<App>, id: String) {
    // A mail chosen inside a conversation leaves that conversation the open row.
    let held = CURRENT.lock().clone().filter(|row| !service.has_row(&id) && service.is_conversation_row(row));
    let staying = held.is_some();
    let row = held.unwrap_or_else(|| id.clone());
    *CURRENT.lock() = Some(row.clone());
    if let Some(window) = app.upgrade() {
        select(&window, &row);
        reader::clear(&window);
        window.set_has_mail(true);
        window.set_opening(true);
        // The webview sits above the skeleton, and still holds the mail being left.
        mail_view::set_visible(false);
    }
    let (service, app) = (service.clone(), app.clone());
    let started = std::time::Instant::now();
    tokio::spawn(async move {
        // Before the row is marked read: where a conversation opens depends on what is unread.
        let target = service.message_of_row(&id).await;
        if !staying {
            let (service, row) = (service.clone(), row.clone());
            tokio::spawn(async move {
                let _ = service.view_select(Some(row)).await;
            });
        }
        let id = target.unwrap_or(id);
        let message = service.message(&id).await;
        let Ok(message) = message.inspect_err(|error| log::warn!("a message did not open: {}", error.kind())) else {
            let _ = app.upgrade_in_event_loop(|app| {
                app.set_opening(false);
                app.set_has_mail(false);
                app.set_failed(true);
            });
            return;
        };
        // Known logos come straight away. One that has to be fetched follows the mail.
        let asked = tokio::time::timeout(LOGO_WAIT, picture_of(&service, &message.meta.from.email)).await;
        let hide = service.mail_settings().await.map_or(true, |settings| settings.hide_remote_images);
        if CURRENT.lock().as_deref() != Some(row.as_str()) {
            return; // the person has already moved on
        }
        OPEN.lock().replace(message.clone());
        LABELS.lock().clone_from(&message.meta.labels);
        let known = KNOWN_TRACKERS.lock().get(&id).cloned();
        *TRACKERS.lock() = known.clone().unwrap_or_default();
        let side = pixels_for(READER_AVATAR);
        // Not known in time: it is looked for again once the mail is on screen.
        let timed_out = asked.is_err();
        let asked = asked.ok().flatten();
        let made = tokio::task::spawn_blocking(move || {
            asked.as_deref().and_then(logo_of).and_then(|logo| picture_pixels(&logo, side))
        });
        let logo_pixels = made.await.ok().flatten();
        let threats = threats_of(&message, &TRACKERS.lock());
        let held_back = service.with_embedded_held_back(&message);
        let (conversation, weak, shown, row_shown) = (service.clone(), app.clone(), message.clone(), row.clone());
        let _ = app.upgrade_in_event_loop(move |app| {
            app.set_sender_has_logo(logo_pixels.is_some());
            if let Some(pixels) = logo_pixels {
                app.set_sender_logo(Image::from_rgba8(pixels));
            }
            app.set_threats(threats);
            app.set_starred(shown.meta.starred);
            let remote = !shown.protection.images.is_empty();
            app.set_images_held(remote || held_back.is_some());
            app.set_images_loading(false);
            fill_reader(&app, &shown, held_back.as_deref());
            reader::fill(&app, &shown);
            log::info!(
                "a message opened in {} ms: {} files, {} pictures inside, {} remote pictures, {} cleaned links, {} threats",
                started.elapsed().as_millis(),
                shown.attachments.iter().filter(|file| !file.inline).count(),
                shown.attachments.iter().filter(|file| file.inline).count(),
                shown.protection.images.len(),
                shown.protection.links.len(),
                shown.protection.threats
            );
            if remote && !hide && held_back.is_none() {
                app.invoke_show_images();
            }
            reader::load_thread(&conversation, &weak, &shown);
            read_ahead(&conversation, &app, &row_shown);
        });
        if timed_out {
            logo_after(&service, &app, &message, &row).await;
        }
        thumbnails_after(&service, &app, &message, &row).await;
        if known.is_none() {
            trackers_after(&service, &app, &message, &row).await;
        }
    });
}

async fn logo_after(service: &Arc<Service>, app: &Weak<App>, message: &Message, id: &str) {
    let logo = picture_of(service, &message.meta.from.email).await;
    let side = pixels_for(READER_AVATAR);
    let made = tokio::task::spawn_blocking(move || {
        logo.as_deref().and_then(logo_of).and_then(|logo| picture_pixels(&logo, side))
    });
    let Some(pixels) = made.await.ok().flatten() else { return };
    let id = id.to_string();
    let _ = app.upgrade_in_event_loop(move |app| {
        if CURRENT.lock().as_deref() == Some(id.as_str()) {
            app.set_sender_has_logo(true);
            app.set_sender_logo(Image::from_rgba8(pixels));
        }
    });
}

/// Each picture of a mail is a question to Proton, so the mail does not wait for them, and the
/// answers are kept for the next time it is opened.
async fn trackers_after(service: &Arc<Service>, app: &Weak<App>, message: &Message, row: &str) {
    let urls: Vec<String> = message.protection.images.iter().map(|image| image.url.clone()).collect();
    if urls.is_empty() {
        return;
    }
    let trackers = service.image_trackers(urls).await;
    {
        let mut known = KNOWN_TRACKERS.lock();
        if known.len() >= TRACKERS_KEPT {
            known.clear();
        }
        known.insert(message.meta.id.clone(), trackers.clone());
    }
    if CURRENT.lock().as_deref() != Some(row) {
        return;
    }
    let threats = threats_of(message, &trackers);
    *TRACKERS.lock() = trackers;
    let _ = app.upgrade_in_event_loop(move |app| app.set_threats(threats));
}

/// The mails on either side are the likely next ones. Reading them here marks nothing as read.
fn read_ahead(service: &Arc<Service>, app: &App, id: &str) {
    let rows = app.get_rows();
    let Some(at) = rows.iter().position(|row| row.id == id) else { return };
    let around = [at.checked_add(1), at.checked_sub(1)];
    for row in around.into_iter().flatten().filter_map(|near| rows.row_data(near)) {
        let (service, near) = (service.clone(), row.id.to_string());
        tokio::spawn(async move {
            if let Ok(near) = service.message_of_row(&near).await {
                let _ = service.message(&near).await;
            }
        });
    }
}

#[cfg(test)]
mod tests;
