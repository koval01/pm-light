//! What belongs to this computer: how the app looks, starts and talks. Appearance, time, the app, the mail cache, the
//! connection, the journal, the keyboard and About.

use super::{
    Ctx,
    rows::{self, ghost, hint, keys, note, plain, record, section, segmented, swatches, switch},
};
use crate::{App, guard, keyboard, look, menu, palette, preferences, startup, tray};
use parking_lot::Mutex;
use proton_core::{
    journal,
    model::{Latency, TimeSetting, TimeSettings},
    timing,
};

pub const LANGUAGES: [(&str, &str); 3] = [("ru", "language.ru"), ("en", "language.en"), ("de", "language.de")];
const THEMES: [&str; 3] = ["settings.themeSystem", "settings.themeLight", "settings.themeDark"];
const PALETTES: [&str; 6] = ["look.blue", "look.graphite", "look.forest", "look.plum", "look.sand", "look.contrast"];
const DENSITIES: [&str; 2] = ["look.comfortable", "look.compact"];
const LAYOUTS: [&str; 2] = ["look.columns", "look.rows"];
const LIST_STYLES: [&str; 2] = ["look.pages", "look.oneList"];
const MAIL_TEXTS: [&str; 3] = ["look.small", "look.normal", "look.large"];
const WEEK_DAYS: [&str; 8] = ["time.weekDefault", "time.day1", "time.day2", "time.day3", "time.day4", "time.day5", "time.day6", "time.day7"];
const CLOCKS: [&str; 3] = ["time.asLanguage", "time.h24", "time.h12"];
const DATE_SAMPLES: [&str; 3] = ["31/12/2026", "12/31/2026", "2026-12-31"];
const READ_LIMIT: usize = 5000;
const SHOWN_LIMIT: usize = 500;
const JOURNAL_LIST: &str = "journal-list";
const SEARCH: &str = "journal-search";

static JOURNAL_LEVEL: Mutex<usize> = Mutex::new(0);

fn words(ctx: &Ctx, keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| ctx.say(key)).collect()
}

fn index(value: i32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

pub fn appearance(ctx: &Ctx) {
    let chosen = preferences::get();
    let zooms: Vec<String> = look::ZOOMS.iter().map(|zoom| format!("{zoom}%")).collect();
    let zoom = look::ZOOMS.iter().position(|zoom| *zoom == chosen.zoom).unwrap_or(1);
    ctx.show(vec![
        section("", &ctx.say("look.colors"), "", vec![
            segmented("theme", &ctx.say("settings.theme"), words(ctx, &THEMES), usize::from(chosen.theme)),
            swatches("palette", &ctx.say("look.palette"), words(ctx, &PALETTES), usize::from(chosen.palette)),
        ]),
        section("", &ctx.say("look.sizes"), "", vec![
            segmented("zoom", &ctx.say("look.zoom"), zooms, zoom),
            segmented("density", &ctx.say("look.density"), words(ctx, &DENSITIES), usize::from(chosen.compact)),
            segmented("mail-text", &ctx.say("look.mailText"), words(ctx, &MAIL_TEXTS), usize::from(chosen.mail_text)),
        ]),
        section("", &ctx.say("look.window"), "", window_rows(ctx, &chosen)),
    ]);
}

fn window_rows(ctx: &Ctx, chosen: &preferences::Preferences) -> Vec<crate::SettingsRow> {
    let mut rows = vec![
        segmented("layout", &ctx.say("look.layout"), words(ctx, &LAYOUTS), usize::from(chosen.rows)),
        segmented("list-style", &ctx.say("look.listStyle"), words(ctx, &LIST_STYLES), usize::from(!crate::list_style::paged())),
        switch("close-to-tray", &ctx.say("look.closeToTray"), "", chosen.close_to_tray),
    ];
    // Elsewhere the icon is the only way back to a closed window, and is not offered to be turned off.
    if cfg!(target_os = "macos") {
        rows.push(switch("menu-bar-icon", &ctx.say("look.menuBarIcon"), &ctx.say("look.menuBarIconHint"), chosen.menu_bar_icon));
    }
    rows
}

fn apply_look(ctx: &Ctx, app: &App, id: &str, value: i32) {
    let chosen = u8::try_from(value).unwrap_or(0);
    preferences::change(|preferences| match id {
        "theme" => preferences.theme = chosen,
        "palette" => preferences.palette = chosen,
        "density" => preferences.compact = value == 1,
        "mail-text" => preferences.mail_text = chosen,
        "layout" => preferences.rows = value == 1,
        "list-style" => preferences.list_pages = Some(value == 0),
        "close-to-tray" => preferences.close_to_tray = value == 1,
        "menu-bar-icon" => preferences.menu_bar_icon = value == 1,
        _ => preferences.zoom = look::ZOOMS.get(index(value)).copied().unwrap_or(100),
    });
    look::apply(app);
    if id == "zoom" {
        look::apply_zoom(app);
    }
    if id == "list-style" {
        let service = ctx.service.clone();
        tokio::spawn(async move { service.view_paged(value == 0).await });
    }
    if id == "menu-bar-icon" {
        tray::install(app);
        tray::show_unread(app, u64::try_from(app.get_inbox_unread()).unwrap_or(0));
    }
    appearance(ctx);
}

fn date_options(ctx: &Ctx) -> Vec<String> {
    let mut options = vec![ctx.say("time.asLanguage")];
    options.extend(DATE_SAMPLES.iter().map(ToString::to_string));
    options
}

fn system_zone() -> String {
    iana_time_zone::get_timezone().unwrap_or_default()
}

fn zone_options(ctx: &Ctx) -> Vec<String> {
    let mut options = vec![ctx.say_with("time.zoneSystem", &[("zone", &system_zone())])];
    options.extend(timing::zones().into_iter().map(|zone| zone.replace('_', " ")));
    options
}

pub fn language(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.time_settings().await }, |ctx, settings: TimeSettings| {
        let current = look::language();
        let selected = LANGUAGES.iter().position(|(code, _)| *code == current).unwrap_or(1);
        let names = LANGUAGES.map(|(_, key)| ctx.say(key)).to_vec();
        let pick = |options: Vec<String>, chosen: u8| options.get(usize::from(chosen)).cloned().unwrap_or_default();
        let zone = preferences::get().time_zone;
        let zone = if zone.is_empty() { ctx.say_with("time.zoneSystem", &[("zone", &system_zone())]) } else { zone.replace('_', " ") };
        ctx.show(vec![
            plain(vec![segmented("language", &ctx.say("settings.language"), names, selected)]),
            section("", &ctx.say("time.title"), "", vec![
                rows::choice("clock", &ctx.say("time.clock"), &pick(words(ctx, &CLOCKS), settings.time_format)),
                rows::choice("date", &ctx.say("time.date"), &pick(date_options(ctx), settings.date_format)),
                rows::choice("week", &ctx.say("time.weekStart"), &pick(words(ctx, &WEEK_DAYS), settings.week_start)),
                rows::choice("zone", &ctx.say("time.zone"), &zone),
                note(&ctx.say_with("time.now", &[("time", &rows::date_time(timing::now_seconds()))])),
                hint(&ctx.say("time.where")),
            ]),
        ]);
    });
}

fn change_time(ctx: &Ctx, app: &App, setting: TimeSetting, options: &[String]) {
    let ctx = ctx.clone();
    tray::choose(app, options, move |_, chosen| {
        let service = ctx.service.clone();
        let value = u8::try_from(chosen).unwrap_or(0);
        ctx.act(async move { service.update_time_setting(setting, value).await }, times_changed);
    });
}

/// The list shows times too, and they are written when its rows are made.
fn times_changed(ctx: &Ctx) {
    language(ctx);
    let service = ctx.service.clone();
    tokio::spawn(async move { service.view_reload().await });
}

fn choose_zone(ctx: &Ctx, app: &App) {
    let ctx = ctx.clone();
    tray::choose(app, &zone_options(&ctx), move |_, chosen| {
        let zone = chosen.checked_sub(1).and_then(|position| timing::zones().get(position).copied()).unwrap_or_default();
        timing::set_zone(zone);
        preferences::change(|preferences| preferences.time_zone = zone.to_string());
        times_changed(&ctx);
    });
}

/// The menus are the system's, so they have to be made again in the new language.
pub fn choose_language(app: &App, code: &str) {
    preferences::change(|preferences| preferences.language = code.to_string());
    look::apply_language();
    menu::install(app);
    tray::install(app);
}

fn pick_language(ctx: &Ctx, app: &App, value: i32) {
    choose_language(app, LANGUAGES.get(index(value)).map_or("en", |(code, _)| code));
    super::language_changed(ctx);
    language(ctx);
}

pub fn application(ctx: &Ctx) {
    let chosen = preferences::get();
    let mut first = vec![switch("notifications", &ctx.say("settings.notifications"), &ctx.say("settings.notificationsHint"), chosen.notifications)];
    if chosen.notifications && crate::notifications::refused_by_the_system() {
        first.push(note(&ctx.say("settings.notificationsRefused")));
        first.push(ghost("system-notifications", &ctx.say("settings.notificationsOpen"), true));
    }
    first.push(switch("autostart", &ctx.say("settings.autostart"), &ctx.say("settings.autostartHint"), startup::enabled()));
    ctx.show(vec![
        plain(first),
        section("", &ctx.say("cache.title"), "", vec![switch("mail-on-disk", &ctx.say("cache.keep"), &ctx.say("cache.about"), chosen.mail_on_disk)]),
        section("", &ctx.say("privacy.screenTitle"), "", vec![
            switch("hide-private", &ctx.say("privacy.hide"), &ctx.say("privacy.hideHint"), chosen.hide_private),
            switch("gravatar", &ctx.say("privacy.gravatar"), &ctx.say("privacy.gravatarHint"), chosen.gravatar),
        ]),
        section("", &ctx.say("tour.title"), "", vec![note(&ctx.say("tour.about")), ghost("tour", &ctx.say("tour.again"), true)]),
    ]);
}

pub fn connection(ctx: &Ctx) {
    let measuring = ctx.say("connection.measuring");
    draw_connection(ctx, &measuring, &measuring, false);
    let service = ctx.service.clone();
    ctx.load(async move { Ok(service.latency().await) }, |ctx, latency: Latency| {
        let shown = |value: Option<u32>| {
            value.map_or_else(|| ctx.say("connection.noAnswer"), |value| ctx.say_with("connection.milliseconds", &[("value", &value.to_string())]))
        };
        draw_connection(ctx, &shown(latency.proton), &shown(latency.quad9), true);
    });
}

fn draw_connection(ctx: &Ctx, proton: &str, quad9: &str, ready: bool) {
    let Some(app) = ctx.app.upgrade() else { return };
    let state = match app.get_link() {
        0 => ctx.say("connection.online"),
        1 => ctx.say("app.offline"),
        2 => ctx.say("app.unavailable"),
        _ => ctx.say("app.untrusted"),
    };
    let mut lines = vec![
        note(&ctx.say("connection.about")),
        rows::fact(&ctx.say("connection.proton"), proton),
        rows::fact(&ctx.say("connection.quad9"), quad9),
        rows::fact(&ctx.say("connection.state"), &state),
    ];
    if !app.get_updated().is_empty() {
        lines.push(rows::fact(&ctx.say("connection.updated"), &app.get_updated()));
    }
    lines.push(rows::button("measure", &ctx.say("connection.measure"), ready));
    ctx.show(vec![plain(lines), section("tor", &ctx.say("connection.torTitle"), "", super::tor_page::summary(ctx, &app))]);
}

fn journal_rows(ctx: &Ctx) -> Vec<crate::SettingsRow> {
    let wanted = ctx.text(SEARCH).trim().to_lowercase();
    let level = *JOURNAL_LEVEL.lock();
    let visible: Vec<_> = journal::read(READ_LIMIT)
        .into_iter()
        .filter(|entry| level == 0 || entry.level == "error" || (level == 1 && entry.level == "warn"))
        .filter(|entry| wanted.is_empty() || format!("{} {}", entry.area, entry.text).to_lowercase().contains(&wanted))
        .take(SHOWN_LIMIT)
        .map(|entry| {
            let urgency = match entry.level.as_str() {
                "error" => 2,
                "warn" => 1,
                _ => 0,
            };
            record(&rows::precise_moment(entry.time), &entry.area, &entry.text, urgency)
        })
        .collect();
    if visible.is_empty() { vec![note(&ctx.say("journal.empty"))] } else { visible }
}

pub fn journal(ctx: &Ctx) {
    if ctx.has_row(SEARCH) {
        ctx.replace(JOURNAL_LIST, journal_rows(ctx));
        return;
    }
    let levels = words(ctx, &["journal.all", "journal.urgent", "journal.errors"]);
    let tools = plain(vec![
        note(&ctx.say("journal.about")),
        segmented("journal-level", "", levels, *JOURNAL_LEVEL.lock()),
        rows::SettingsRow { detail: ctx.say("journal.search").into(), ..rows::field(SEARCH, "", "") },
        ghost("journal-refresh", &ctx.say("journal.refresh"), true),
        ghost("journal-clear", &ctx.say("journal.clear"), true),
    ]);
    ctx.show(vec![tools, section(JOURNAL_LIST, "", "", journal_rows(ctx))]);
}

fn clear_journal(ctx: &Ctx) {
    let title = ctx.say("journal.clear");
    if crate::actions::confirmed(&title, &ctx.say("journal.clearText"), &title, &ctx.say("common.cancel")) {
        journal::clear();
        ctx.replace(JOURNAL_LIST, journal_rows(ctx));
    }
}

pub fn keyboard(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.mail_settings().await }, |ctx, settings| {
        let Some(app) = ctx.app.upgrade() else { return };
        let mut lines = vec![switch("shortcuts", &ctx.say("settings.shortcuts"), "", settings.shortcuts)];
        if settings.shortcuts {
            let command = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };
            let (chords, plain_keys): (Vec<_>, Vec<_>) = palette::shortcut_rows(&app).into_iter().partition(|(chord, ..)| *chord);
            lines.extend(plain_keys.iter().map(|(_, label, shortcut)| keys(label, shortcut)));
            lines.push(keys(&ctx.say("shortcut.sendMail"), &format!("{command} Enter")));
            lines.push(keys(&ctx.say("shortcut.saveDraft"), &format!("{command} S")));
            lines.push(rows::strong(&ctx.say("settings.navigation")));
            lines.extend(chords.iter().map(|(_, label, shortcut)| keys(label, shortcut)));
        }
        ctx.show(vec![section("", &ctx.say("settings.keyboard"), "", lines)]);
    });
}

pub fn heard(ctx: &Ctx, tab: &str, id: &str, value: i32) {
    let Some(app) = ctx.app.upgrade() else { return };
    match id {
        "theme" | "palette" | "zoom" | "density" | "mail-text" | "layout" | "close-to-tray" | "menu-bar-icon" => apply_look(ctx, &app, id, value),
        "language" => pick_language(ctx, &app, value),
        "clock" => change_time(ctx, &app, TimeSetting::TimeFormat, &words(ctx, &CLOCKS)),
        "date" => change_time(ctx, &app, TimeSetting::DateFormat, &date_options(ctx)),
        "week" => change_time(ctx, &app, TimeSetting::WeekStart, &words(ctx, &WEEK_DAYS)),
        "zone" => choose_zone(ctx, &app),
        "system-notifications" => crate::view::open_notification_settings(),
        "notifications" => {
            preferences::change(|preferences| preferences.notifications = value == 1);
            application(ctx);
        }
        "autostart" => {
            if !startup::set(value == 1) {
                ctx.tell(&ctx.say("errors.internal"));
            }
            application(ctx);
        }
        "gravatar" => {
            let on = value == 1;
            let say = |key: &str| ctx.say(key);
            if on && !crate::actions::confirmed(&say("privacy.gravatarTitle"), &say("privacy.gravatarText"), &say("privacy.gravatarYes"), &say("common.cancel")) {
                application(ctx);
                return;
            }
            preferences::change(|preferences| preferences.gravatar = on);
            log::info!("Gravatar pictures: {}", if on { "on" } else { "off" });
            crate::view::forget_pictures();
            // The rows already shown look for their pictures again, as a refresh does.
            app.invoke_reload();
            application(ctx);
        }
        "hide-private" => {
            preferences::change(|preferences| preferences.hide_private = value == 1);
            log::info!("private details: {}", if value == 1 { "covered" } else { "shown" });
            look::dress(&slint::ComponentHandle::global::<crate::Theme>(&app));
            application(ctx);
        }
        "mail-on-disk" => {
            preferences::change(|preferences| preferences.mail_on_disk = value == 1);
            guard::use_mail_store(&ctx.service);
            application(ctx);
        }
        "measure" => connection(ctx),
        "tour" => {
            super::close_for(ctx);
            crate::tour::start(&app);
        }
        "open-tor" => super::open_tab(ctx, "tor"),
        "journal-level" => {
            *JOURNAL_LEVEL.lock() = index(value);
            ctx.replace(JOURNAL_LIST, journal_rows(ctx));
        }
        "journal-refresh" => ctx.replace(JOURNAL_LIST, journal_rows(ctx)),
        "journal-clear" => clear_journal(ctx),
        "shortcuts" => {
            let (service, on) = (ctx.service.clone(), value == 1);
            keyboard::allow(on);
            ctx.act(async move { service.update_mail_setting("shortcuts", on).await }, keyboard);
        }
        _ => log::debug!("nothing listens on {tab}"),
    }
}
