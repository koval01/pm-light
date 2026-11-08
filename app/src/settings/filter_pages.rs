//! Filters, simple or in Sieve, and the lists of senders. Sieve itself is checked by Proton when a filter is saved.

use super::{
    Ctx,
    rows::{SettingsRow, area, button, choice, code_lines, error, field, filter, note, section, segmented, switch},
};
use crate::tray;
use parking_lot::Mutex;
use proton_core::{
    model::{FilterDraft, Folder, MailFilter, SenderList, SenderRule},
    sieve::SimpleFilter,
};

const FILTERS: &str = "filters";
const SENDERS: &str = "senders-list";
const NAME: &str = "filter-name";
const VALUE: &str = "filter-value";
const SIEVE: &str = "filter-sieve";
const SAVE: &str = "filter-save";
const SENDER: &str = "sender";
const SENDER_ADD: &str = "sender-add";
const SENDER_SEARCH: &str = "sender-search";
const SIEVE_LINES: i32 = 12;
const SHOWN_LIMIT: usize = 200;
const FIELDS: [(&str, &str); 3] = [("from", "settings.filterFrom"), ("to", "settings.filterTo"), ("subject", "settings.filterSubject")];
const LISTS: [(SenderList, &str); 3] = [(SenderList::Spam, "senders.spam"), (SenderList::Blocked, "senders.blocked"), (SenderList::Allowed, "senders.allowed")];
const TARGETS: [&str; 3] = ["archive", "trash", "spam"];
/// Shown in the Sieve form until the person writes their own.
const STARTER: (&str, &str) = ("from", "someone@example.com");

#[derive(Default)]
struct State {
    filters: Vec<MailFilter>,
    folders: Vec<Folder>,
    rules: Vec<SenderRule>,
    sieve_form: bool,
    editing: Option<MailFilter>,
    /// What the fields hold, so that drawing the page again keeps it.
    name: String,
    value: String,
    sieve: String,
    field: usize,
    target: usize,
    mark_read: bool,
    star: bool,
    issues: Vec<String>,
    add_to: usize,
    shown: usize,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(read: impl FnOnce(&mut State) -> T) -> T {
    read(STATE.lock().get_or_insert_with(State::default))
}

fn word_for(ctx: &Ctx, key: &str) -> String {
    ctx.say(key)
}

fn targets(state: &State) -> Vec<&Folder> {
    state.folders.iter().filter(|folder| TARGETS.contains(&folder.id.as_str()) || folder.kind != "system").collect()
}

fn target_names(ctx: &Ctx, state: &State) -> Vec<String> {
    let mut names = vec![ctx.say("settings.filterKeep")];
    names.extend(targets(state).into_iter().map(|folder| folder.name.clone()));
    names
}

fn target_of(state: &State) -> Option<String> {
    let folder = *targets(state).get(state.target.checked_sub(1)?)?;
    Some(if folder.kind == "system" { folder.id.clone() } else { folder.name.clone() })
}

fn simple_form(ctx: &Ctx, state: &State) -> Vec<SettingsRow> {
    let field_name = FIELDS.get(state.field).map_or_else(String::new, |(_, key)| ctx.say(key));
    let target = target_names(ctx, state).get(state.target).cloned().unwrap_or_default();
    vec![
        field(NAME, &ctx.say("settings.filterName"), &state.name),
        choice("filter-field", &ctx.say("settings.filterIf"), &field_name),
        field(VALUE, &ctx.say("settings.filterContains"), &state.value),
        choice("filter-target", &ctx.say("settings.filterThen"), &target),
        switch("filter-read", &ctx.say("settings.filterMarkRead"), "", state.mark_read),
        switch("filter-star", &ctx.say("settings.filterStar"), "", state.star),
    ]
}

fn sieve_form(ctx: &Ctx, state: &State) -> Vec<SettingsRow> {
    vec![field(NAME, &ctx.say("settings.filterName"), &state.name), SettingsRow { mono: true, code: code_lines(&state.sieve), ..area(SIEVE, "Sieve", &state.sieve, SIEVE_LINES) }]
}

fn ready(state: &State, name: &str, value: &str, sieve: &str) -> bool {
    if state.editing.is_some() || state.sieve_form {
        return !name.trim().is_empty() && !sieve.trim().is_empty();
    }
    let acts = state.target > 0 || state.mark_read || state.star;
    !name.trim().is_empty() && !value.trim().is_empty() && acts
}

fn filter_rows(ctx: &Ctx, state: &State) -> Vec<SettingsRow> {
    let mut lines: Vec<SettingsRow> = state.filters.iter().map(|known| filter(&format!("filter:{}", known.id), &known.name, known.enabled)).collect();
    if lines.is_empty() {
        lines.push(note(&ctx.say("settings.filterNone")));
    }
    if state.editing.is_some() {
        lines.extend(sieve_form(ctx, state));
    } else {
        let modes = vec![ctx.say("settings.filterModeSimple"), "Sieve".to_string()];
        lines.push(segmented("filter-mode", &ctx.say("settings.filterAdd"), modes, usize::from(state.sieve_form)));
        lines.extend(if state.sieve_form { sieve_form(ctx, state) } else { simple_form(ctx, state) });
    }
    lines.extend(state.issues.iter().map(|issue| error(issue)));
    let label = if state.editing.is_some() { "common.save" } else { "settings.filterAdd" };
    let mut save = button(SAVE, &ctx.say(label), ready(state, &state.name, &state.value, &state.sieve));
    if state.editing.is_some() {
        save.second = ctx.say("common.cancel").into();
    }
    lines.push(save);
    lines
}

fn sender_rows(ctx: &Ctx, state: &State) -> Vec<SettingsRow> {
    let wanted = ctx.text(SENDER_SEARCH).trim().to_lowercase();
    let list = state.shown.checked_sub(1).and_then(|position| LISTS.get(position)).map(|(list, _)| *list);
    let visible: Vec<&SenderRule> = state.rules.iter().filter(|rule| list.is_none_or(|list| rule.list == list)).filter(|rule| rule.sender.to_lowercase().contains(&wanted)).collect();
    let mut lines: Vec<SettingsRow> = visible
        .iter()
        .take(SHOWN_LIMIT)
        .map(|rule| {
            let label = LISTS.iter().find(|(list, _)| *list == rule.list).map_or("", |(_, key)| key);
            SettingsRow {
                id: format!("rule:{}", rule.id).into(),
                title: rule.sender.clone().into(),
                tag: ctx.say(label).into(),
                button: ctx.say("senders.remove").into(),
                ..note("")
            }
        })
        .map(super::rows::as_entry)
        .collect();
    if lines.is_empty() {
        lines.push(note(&ctx.say("senders.empty")));
    }
    if visible.len() > SHOWN_LIMIT {
        lines.push(note(&ctx.say_with("senders.more", &[("count", &(visible.len() - SHOWN_LIMIT).to_string())])));
    }
    lines
}

fn draw(ctx: &Ctx) {
    with(|state| {
        let names: Vec<String> = LISTS.iter().map(|(_, key)| word_for(ctx, key)).collect();
        let mut all = vec![ctx.say("journal.all")];
        all.extend(names.iter().cloned());
        ctx.show(vec![
            section(FILTERS, &ctx.say("settings.filters"), "", filter_rows(ctx, state)),
            section("senders", &ctx.say("senders.title"), "", vec![
                note(&ctx.say("senders.about")),
                SettingsRow { detail: ctx.say("senders.placeholder").into(), ..field(SENDER, "", "") },
                segmented("sender-list", &ctx.say("senders.list"), names, state.add_to),
                button(SENDER_ADD, &ctx.say("senders.add"), false),
                segmented("sender-show", &ctx.say("senders.show"), all, state.shown),
                SettingsRow { detail: ctx.say("senders.search").into(), ..field(SENDER_SEARCH, "", "") },
            ]),
            section(SENDERS, "", "", sender_rows(ctx, state)),
        ]);
    });
}

pub fn filters(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move {
        let filters = service.filters().await?;
        let folders = service.folders().await.unwrap_or_default();
        Ok((filters, folders, service.sender_rules().await.unwrap_or_default()))
    };
    let starter = ctx.service.build_sieve(&SimpleFilter { field: STARTER.0.to_string(), value: STARTER.1.to_string(), target: Some("archive".to_string()), mark_read: false, star: false });
    ctx.load(task, move |ctx, (filters, folders, rules): (Vec<MailFilter>, Vec<Folder>, Vec<SenderRule>)| {
        with(|state| {
            state.filters = filters;
            state.folders = folders;
            state.rules = rules;
            if state.sieve.is_empty() {
                state.sieve = starter;
            }
        });
        draw(ctx);
    });
}

/// Keeps what was typed, then shows the form as it now stands.
fn form_changed(ctx: &Ctx, change: impl FnOnce(&mut State)) {
    keep_fields(ctx);
    with(change);
    with(|state| ctx.replace(FILTERS, filter_rows(ctx, state)));
}

fn keep_fields(ctx: &Ctx) {
    let (name, value, sieve) = (ctx.text(NAME), ctx.text(VALUE), ctx.text(SIEVE));
    let (has_value, has_sieve) = (ctx.has_row(VALUE), ctx.has_row(SIEVE));
    with(|state| {
        state.name = name;
        if has_value {
            state.value = value;
        }
        if has_sieve {
            state.sieve = sieve;
        }
    });
}

fn saved(ctx: &Ctx, issues: Vec<String>) {
    if !issues.is_empty() {
        form_changed(ctx, |state| state.issues = issues);
        return;
    }
    with(|state| {
        state.editing = None;
        state.name.clear();
        state.value.clear();
        state.issues.clear();
    });
    ctx.tell(&ctx.say("settings.filterSaved"));
    filters(ctx);
}

fn save(ctx: &Ctx) {
    keep_fields(ctx);
    let draft = with(|state| {
        let sieve = if state.editing.is_some() || state.sieve_form {
            state.sieve.clone()
        } else {
            let field = FIELDS.get(state.field).map_or("from", |(name, _)| name).to_string();
            ctx.service.build_sieve(&SimpleFilter { field, value: state.value.clone(), target: target_of(state), mark_read: state.mark_read, star: state.star })
        };
        FilterDraft { id: state.editing.as_ref().map(|filter| filter.id.clone()), enabled: state.editing.as_ref().map(|filter| filter.enabled), name: state.name.trim().to_string(), sieve }
    });
    let service = ctx.service.clone();
    ctx.load(async move { service.save_filter(&draft).await }, saved);
}

fn filter_event(ctx: &Ctx, id: &str, part: i32, value: i32) {
    let (service, id) = (ctx.service.clone(), id.to_string());
    match part {
        0 => {
            let found = with(|state| state.filters.iter().find(|known| known.id == id).cloned());
            let Some(found) = found else { return };
            form_changed(ctx, |state| {
                state.name.clone_from(&found.name);
                state.sieve.clone_from(&found.sieve);
                state.issues.clear();
                state.editing = Some(found);
            });
        }
        1 => ctx.act(async move { service.delete_filter(&id).await }, filters),
        _ => ctx.act(async move { service.set_filter_enabled(&id, value == 1).await }, filters),
    }
}

fn pick(ctx: &Ctx, options: &[String], keep: fn(&mut State, usize)) {
    let Some(app) = ctx.app.upgrade() else { return };
    let ctx = ctx.clone();
    tray::choose(&app, options, move |_, chosen| form_changed(&ctx, |state| keep(state, chosen)));
}

fn add_sender(ctx: &Ctx) {
    let (service, sender) = (ctx.service.clone(), ctx.text(SENDER));
    let list = with(|state| LISTS.get(state.add_to).map_or(SenderList::Spam, |(list, _)| *list));
    ctx.act(async move { service.add_sender_rule(&sender, list).await }, filters);
}

pub fn heard(ctx: &Ctx, id: &str, part: i32, value: i32) {
    let position = usize::try_from(value).unwrap_or(0);
    match id {
        "filter-mode" => form_changed(ctx, |state| state.sieve_form = value == 1),
        "filter-read" => form_changed(ctx, |state| state.mark_read = value == 1),
        "filter-star" => form_changed(ctx, |state| state.star = value == 1),
        "filter-field" => pick(ctx, &FIELDS.iter().map(|(_, key)| ctx.say(key)).collect::<Vec<_>>(), |state, chosen| state.field = chosen),
        "filter-target" => {
            let names = with(|state| target_names(ctx, state));
            pick(ctx, &names, |state, chosen| state.target = chosen);
        }
        SAVE if part == 1 => form_changed(ctx, |state| {
            state.editing = None;
            state.name.clear();
            state.issues.clear();
        }),
        SAVE => save(ctx),
        "sender-list" => with(|state| state.add_to = position),
        "sender-show" => {
            with(|state| state.shown = position);
            with(|state| ctx.replace(SENDERS, sender_rows(ctx, state)));
        }
        SENDER_ADD => add_sender(ctx),
        _ => {
            let service = ctx.service.clone();
            if let Some(found) = id.strip_prefix("filter:") {
                filter_event(ctx, found, part, value);
            } else if let Some(rule) = id.strip_prefix("rule:") {
                let rule = rule.to_string();
                ctx.act(async move { service.remove_sender_rule(&rule).await }, filters);
            }
        }
    }
}

pub fn typed(ctx: &Ctx, id: &str) {
    if id == SIEVE {
        ctx.recolour(SIEVE);
    }
    match id {
        SENDER_SEARCH => with(|state| ctx.replace(SENDERS, sender_rows(ctx, state))),
        SENDER => ctx.enable(SENDER_ADD, !ctx.text(SENDER).trim().is_empty()),
        _ => {
            let enabled = with(|state| ready(state, &ctx.text(NAME), &ctx.text(VALUE), &ctx.text(SIEVE)));
            ctx.enable(SAVE, enabled);
        }
    }
}
