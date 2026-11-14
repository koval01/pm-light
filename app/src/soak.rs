//! The long run: after the account check drew its mail, the window is driven at random for as long as it was told,
//! `--soak-minutes=<n>`, through what a person does with it. Mail is opened, stepped through and scrolled; folders,
//! search, settings, letters, previews, the palette and contacts are opened and left; the look, the zoom and the
//! window's size change; the list is reloaded. A seed, `--soak-seed=<n>` or the clock's, makes a run repeatable.
//!
//! Every minute the memory the app takes is written down, and at the end `<file>.soak` holds the minutes, the actions
//! taken, the memory of each minute, and every request to Proton summed up by `proton_core::request_log`: numbers and
//! paths without ids only. CI judges them (`scripts/soak-report.sh`). Nothing of the account changes: a check refuses
//! every request but a read (`checks`). `--soak-tor` takes the way through Tor, for CI to see that nothing passes it.

use crate::{App, Settings, Theme, composer, contacts, look, memory, palette, preferences, settings, view};
use proton_core::{Service, model::MessageMeta, request_log::RequestLog};
use rand::{Rng, SeedableRng, rngs::StdRng, seq::SliceRandom};
use slint::{ComponentHandle, LogicalSize, Timer, TimerMode, Weak};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

const MINUTES: &str = "--soak-minutes=";
const SEED: &str = "--soak-seed=";
const THROUGH_TOR: &str = "--soak-tor";
/// Between two actions: a person's pace, from quick to unhurried.
const FASTEST: u64 = 500;
const SLOWEST: u64 = 2500;
const MINUTE: Duration = Duration::from_secs(60);
const FOLDERS: [&str; 10] =
    ["inbox", "starred", "drafts", "scheduled", "snoozed", "sent", "archive", "spam", "trash", "all"];
/// Searched for, beside a word from a subject in the inbox, whatever its script.
const WORDS: [&str; 4] = ["sale", "plan", "invoice", "report"];
const LETTERS: [&str; 4] = ["new", "reply", "replyAll", "forward"];
/// Mail acts the server refuses in a check, so that the refusal is driven too.
const MAIL_ACTS: [&str; 3] = ["star", "unread", "archive"];

/// What the run does next, each as a person would.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Action {
    OpenMail,
    Step,
    Scroll,
    Folder,
    Search,
    Settings,
    Letter,
    Preview,
    Palette,
    Contacts,
    Look,
    Zoom,
    Resize,
    Maximise,
    Reload,
    ShowImages,
    MailAct,
}

/// How often each is chosen, against the others: what is done most with a mail client comes up most.
const WEIGHTS: [(Action, u32); 17] = [
    (Action::OpenMail, 12),
    (Action::Step, 8),
    (Action::Scroll, 6),
    (Action::Folder, 6),
    (Action::Search, 4),
    (Action::Settings, 5),
    (Action::Letter, 3),
    (Action::Preview, 2),
    (Action::Palette, 2),
    (Action::Contacts, 2),
    (Action::Look, 2),
    (Action::Zoom, 2),
    (Action::Resize, 3),
    (Action::Maximise, 2),
    (Action::Reload, 4),
    (Action::ShowImages, 2),
    (Action::MailAct, 2),
];

fn argument(prefix: &str) -> Option<String> {
    std::env::args().skip(1).find_map(|argument| argument.strip_prefix(prefix).map(str::to_string))
}

/// The minutes asked for, when this start is a long run.
pub fn minutes() -> Option<u64> {
    argument(MINUTES).and_then(|text| text.parse().ok()).filter(|minutes| *minutes > 0)
}

/// Whether the long run goes through Tor.
pub fn through_tor() -> bool {
    std::env::args().skip(1).any(|argument| argument == THROUGH_TOR)
}

struct Soak {
    service: Arc<Service>,
    app: Weak<App>,
    rows: Vec<MessageMeta>,
    random: StdRng,
    seed: u64,
    until: Instant,
    started: Instant,
    taken: BTreeMap<Action, u64>,
    memory: Vec<u64>,
    requests: Arc<RequestLog>,
    note: PathBuf,
    /// Something open that the next action puts away first: a letter, settings, a preview, the palette, contacts.
    open: Option<Action>,
    timer: Timer,
    clock: Timer,
}

thread_local! {
    static SOAK: RefCell<Option<Soak>> = const { RefCell::new(None) };
}

fn beside(note: &Path, ending: &str) -> PathBuf {
    let mut name = note.as_os_str().to_os_string();
    name.push(ending);
    PathBuf::from(name)
}

/// From the account check, once its note is written.
pub fn start(app: &App, note: PathBuf, minutes: u64) {
    let Some((service, rows)) = crate::screen_tour::take_seen() else { return };
    let seed = argument(SEED).and_then(|text| text.parse().ok()).unwrap_or_else(|| {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
    });
    log::info!("the long run: {minutes} minutes, seed {seed}, {} mails", rows.len());
    let requests = service.record_requests();
    let soak = Soak {
        service,
        app: app.as_weak(),
        rows,
        random: StdRng::seed_from_u64(seed),
        seed,
        until: Instant::now() + MINUTE * u32::try_from(minutes).unwrap_or(u32::MAX),
        started: Instant::now(),
        taken: BTreeMap::new(),
        memory: Vec::new(),
        requests,
        note,
        open: None,
        timer: Timer::default(),
        clock: Timer::default(),
    };
    soak.clock.start(TimerMode::Repeated, MINUTE, measure);
    SOAK.set(Some(soak));
    next_after_a_while();
}

fn next_after_a_while() {
    SOAK.with_borrow_mut(|soak| {
        if let Some(soak) = soak {
            let pause = Duration::from_millis(soak.random.gen_range(FASTEST..=SLOWEST));
            soak.timer.start(TimerMode::SingleShot, pause, act);
        }
    });
}

fn measure() {
    SOAK.with_borrow_mut(|soak| {
        if let Some(soak) = soak {
            let megabytes = memory::settled_megabytes().unwrap_or(0);
            soak.memory.push(megabytes);
            let asked = soak.requests.summary().total;
            log::info!("the long run: minute {}, {megabytes} MB, {asked} requests so far", soak.memory.len());
        }
    });
}

/// One action, chosen at random by weight, after whatever was left open is put away.
fn act() {
    let chosen = SOAK.with_borrow_mut(|soak| {
        let soak = soak.as_mut()?;
        if Instant::now() >= soak.until {
            return None;
        }
        let action = WEIGHTS.choose_weighted(&mut soak.random, |(_, weight)| *weight).ok()?.0;
        *soak.taken.entry(action).or_default() += 1;
        let pick = soak.random.r#gen::<u64>();
        Some((soak.service.clone(), soak.app.upgrade()?, soak.rows.clone(), soak.open.take(), action, pick))
    });
    let Some((service, app, rows, open, action, pick)) = chosen else {
        finish();
        return;
    };
    if let Some(open) = open {
        put_away(&app, open);
    }
    let left_open = perform(&service, &app, &rows, action, pick);
    SOAK.with_borrow_mut(|soak| {
        if let Some(soak) = soak {
            soak.open = left_open.then_some(action);
        }
    });
    next_after_a_while();
}

fn one_of<'a>(list: &[&'a str], pick: u64) -> &'a str {
    let Some(count) = u64::try_from(list.len()).ok().filter(|count| *count > 0) else { return "" };
    usize::try_from(pick % count).ok().and_then(|at| list.get(at)).copied().unwrap_or_default()
}

/// A word of a mail's subject to search for, in whatever script the mail is written.
fn subject_word(row: &MessageMeta) -> Option<&str> {
    row.subject.split_whitespace().find(|word| word.chars().count() >= 3)
}

/// Does `action`; true when it left something open for the next one to put away.
fn perform(service: &Arc<Service>, app: &App, rows: &[MessageMeta], action: Action, pick: u64) -> bool {
    let window = app.window();
    match action {
        Action::OpenMail => {
            let at = usize::try_from(pick % u64::try_from(rows.len().max(1)).unwrap_or(1)).unwrap_or(0);
            if let Some(row) = rows.get(at) {
                app.invoke_open_folder("inbox".into());
                view::open_mail(service, &app.as_weak(), row.id.clone());
            }
        }
        Action::Step => view::step(service, &app.as_weak(), if pick.is_multiple_of(2) { 1 } else { -1 }),
        Action::Scroll => crate::keyboard::scroll(app, if pick.is_multiple_of(2) { 1 } else { -1 }),
        Action::Folder => app.invoke_open_folder(one_of(&FOLDERS, pick).into()),
        Action::Search => {
            let from_inbox = rows.get(usize::try_from(pick / 2 % 64).unwrap_or(0)).and_then(subject_word);
            let word = if pick.is_multiple_of(2) { from_inbox.unwrap_or("sale") } else { one_of(&WORDS, pick) };
            app.set_query_text(word.into());
            app.invoke_search(word.into());
            return true;
        }
        Action::Settings => {
            let tabs: Vec<&str> = settings::tabs::all().map(|tab| tab.id).collect();
            settings::open(service, app, one_of(&tabs, pick));
            return true;
        }
        Action::Letter => {
            app.invoke_compose(one_of(&LETTERS, pick).into());
            return true;
        }
        Action::Preview => {
            app.invoke_open_file(i32::from(pick.is_multiple_of(2)));
            return true;
        }
        Action::Palette => {
            palette::open(service, app);
            return true;
        }
        Action::Contacts => {
            contacts::open(service, app);
            return true;
        }
        Action::Look => {
            let theme = app.global::<Theme>();
            theme.set_choice(i32::try_from(pick % 3).unwrap_or(0));
            theme.set_palette(i32::try_from((pick / 3) % 6).unwrap_or(0));
        }
        Action::Zoom => {
            let zoom = look::ZOOMS.get(usize::try_from(pick % 5).unwrap_or(1)).copied().unwrap_or(100);
            preferences::change(|chosen| chosen.zoom = zoom);
            look::apply_zoom(app);
        }
        Action::Resize => {
            let width = 900.0 + crate::numbers::float_of(usize::try_from(pick % 700).unwrap_or(0));
            let height = 580.0 + crate::numbers::float_of(usize::try_from((pick / 700) % 420).unwrap_or(0));
            window.set_maximized(false);
            window.set_size(LogicalSize::new(width, height));
        }
        Action::Maximise => window.set_maximized(!window.is_maximized()),
        Action::Reload => app.invoke_reload(),
        Action::ShowImages => app.invoke_show_images(),
        Action::MailAct => app.invoke_act(one_of(&MAIL_ACTS, pick).into()),
    }
    false
}

/// What the previous action left open goes, as a person would close it.
fn put_away(app: &App, open: Action) {
    match open {
        Action::Search => {
            app.set_query_text("".into());
            app.invoke_search("".into());
        }
        Action::Settings => app.global::<Settings>().invoke_back(),
        Action::Letter => composer::abandon(),
        Action::Preview => crate::preview::close(),
        Action::Palette => {
            if let Some(service) = crate::actions::service() {
                palette::open(&service, app);
            }
        }
        Action::Contacts => contacts::close(),
        _ => {}
    }
}

/// What `<file>.soak` holds: numbers, and paths without ids.
fn report(soak: &Soak) -> String {
    let mut text = String::new();
    let minutes = soak.started.elapsed().as_secs() / 60;
    let _ = writeln!(text, "minutes={minutes}\nseed={}", soak.seed);
    for (action, count) in &soak.taken {
        let _ = writeln!(text, "action {action:?}={count}");
    }
    let figures: Vec<String> = soak.memory.iter().map(u64::to_string).collect();
    let _ = writeln!(text, "memory MB={}", figures.join(","));
    let summary = soak.requests.summary();
    let _ = writeln!(
        text,
        "requests={}\nrefused={}\nmost in a minute={}",
        summary.total, summary.refused, summary.most_in_a_minute
    );
    for (kind, count) in &summary.by_kind {
        let _ = writeln!(text, "asked {kind}={count}");
    }
    for (kind, count) in &summary.repeats {
        let _ = writeln!(text, "repeated {kind}={count}");
    }
    text
}

fn finish() {
    let Some(soak) = SOAK.take() else { return };
    soak.clock.stop();
    let text = report(&soak);
    log::info!("the long run ended after {} actions", soak.taken.values().sum::<u64>());
    let _ = std::fs::write(beside(&soak.note, ".soak"), text);
    let _ = std::fs::write(beside(&soak.note, ".done"), "done\n");
}

#[cfg(test)]
mod tests;
