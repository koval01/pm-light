//! Settings of the mail itself: the mail settings, addresses, folders, forwarding and the automatic reply.

use super::{
    Ctx,
    rows::{SettingsRow, area, button, card, choice, entry, fact, field, ghost, note, plain, section, segmented, switch},
};
use crate::{SettingsSection, actions, auth, moment_picker, tray};
use parking_lot::Mutex;
use proton_core::{
    Error, Service,
    mapping::{FONT_FACES, FONT_SIZES},
    model::{AutoReply, Folder, Forwarding, ForwardingState, MailSettings},
    timing,
};
use std::{collections::HashMap, sync::Arc};

/// Where each setting of the table stands.
pub type Choices = HashMap<&'static str, usize>;

/// `DELAY_IN_SECONDS` in Proton's client.
const UNDO_SECONDS: [u64; 4] = [0, 5, 10, 20];
const REPLY_SUBJECT: &str = "reply-subject";
const REPLY_TEXT: &str = "reply-text";
const REPLY_FROM: &str = "reply-from";
const REPLY_TO: &str = "reply-to";
const NEW_NAME: &str = "folder-name";
const NEW_SAVE: &str = "folder-save";

/// The auto-reply as it is being edited, and which folder or label is.
static REPLY: Mutex<Option<AutoReply>> = Mutex::new(None);
static EDITING: Mutex<Option<Editing>> = Mutex::new(None);

#[derive(Clone)]
enum Editing {
    New(&'static str),
    Rename { id: String, color: String, name: String },
}

fn mail_switch(ctx: &Ctx, settings: &MailSettings, key: &str, label: &str, hint: &str) -> SettingsRow {
    let on = match key {
        "nextMessageOnMove" => settings.next_message_on_move,
        "autoDeleteOld" => settings.auto_delete_old,
        "hideRemoteImages" => settings.hide_remote_images,
        "confirmLinks" => settings.confirm_links,
        _ => settings.auto_save_contacts,
    };
    let hint = if hint.is_empty() { String::new() } else { ctx.say(hint) };
    switch(key, &ctx.say(label), &hint, on)
}

fn undo_row(ctx: &Ctx, settings: &MailSettings) -> SettingsRow {
    let undo: Vec<String> = UNDO_SECONDS
        .iter()
        .map(|seconds| match seconds {
            0 => ctx.say("settings.undoOff"),
            count => ctx.say_with("settings.undoSeconds", &[("count", &count.to_string())]),
        })
        .collect();
    let selected = UNDO_SECONDS.iter().position(|seconds| *seconds == settings.delay_send_seconds).unwrap_or(0);
    segmented("undo", &ctx.say("settings.undoSend"), undo, selected)
}

/// A yes or no from the table of settings.
fn flag(ctx: &Ctx, choices: &Choices, key: &str, label: &str, hint: &str) -> SettingsRow {
    let hint = if hint.is_empty() { String::new() } else { ctx.say(hint) };
    switch(&format!("choice:{key}"), &ctx.say(label), &hint, choices.get(key) == Some(&1))
}

/// On a plan that decides for the account, the switch shows what holds and cannot be moved.
fn footer_row(ctx: &Ctx) -> SettingsRow {
    let habits = ctx.service.habits();
    let hint = ctx.say("mail.protonFooterHint");
    let line = switch("choice:protonFooter", &ctx.say("mail.protonFooter"), &hint, habits.proton_footer);
    SettingsRow { enabled: !habits.proton_footer_locked, ..line }
}

fn pick(ctx: &Ctx, choices: &Choices, key: &str, label: &str, options: &[&str]) -> SettingsRow {
    let names = options.iter().map(|option| ctx.say(option)).collect();
    segmented(&format!("choice:{key}"), &ctx.say(label), names, choices.get(key).copied().unwrap_or(0))
}

fn listed(ctx: &Ctx, choices: &Choices, key: &str, label: &str, names: &[String]) -> SettingsRow {
    let name = choices.get(key).and_then(|position| names.get(*position)).cloned().unwrap_or_else(|| ctx.say("mail.asDefault"));
    choice(&format!("list:{key}"), &ctx.say(label), &name)
}

fn font_faces() -> Vec<String> {
    FONT_FACES.iter().map(|face| face.split(',').next().unwrap_or(face).to_string()).collect()
}

fn font_sizes() -> Vec<String> {
    FONT_SIZES.iter().map(|size| format!("{size} px")).collect()
}

async fn both(service: Arc<Service>) -> Result<(MailSettings, Choices), Error> {
    Ok((service.mail_settings().await?, service.mail_choices().await?))
}

pub fn messages(ctx: &Ctx) {
    ctx.load(both(ctx.service.clone()), |ctx, (settings, choices): (MailSettings, Choices)| {
        ctx.show(vec![
            section("", &ctx.say("mail.layout"), "", vec![
                pick(ctx, &choices, "viewMode", "mail.viewMode", &["mail.conversations", "mail.singleMessages"]),
                pick(ctx, &choices, "pageSize", "mail.pageSize", &["mail.page50", "mail.page100", "mail.page200"]),
                flag(ctx, &choices, "stickyLabels", "mail.stickyLabels", "mail.stickyLabelsHint"),
                flag(ctx, &choices, "almostAllMail", "mail.almostAllMail", "mail.almostAllMailHint"),
                flag(ctx, &choices, "folderColor", "mail.folderColor", ""),
                flag(ctx, &choices, "inheritFolderColor", "mail.inheritFolderColor", ""),
            ]),
            section("", &ctx.say("settings.reading"), "", vec![
                mail_switch(ctx, &settings, "nextMessageOnMove", "settings.nextOnMove", ""),
                mail_switch(ctx, &settings, "autoDeleteOld", "settings.autoDelete", "settings.autoDeleteHint"),
                pick(ctx, &choices, "showMoved", "mail.showMoved", &["mail.movedNone", "mail.movedDrafts", "mail.movedSent", "mail.movedBoth"]),
            ]),
            section("", &ctx.say("settings.writing"), "", vec![
                mail_switch(ctx, &settings, "autoSaveContacts", "settings.autoSaveContacts", ""),
                undo_row(ctx, &settings),
                pick(ctx, &choices, "draftType", "mail.draftType", &["mail.draftRich", "mail.draftPlain"]),
                pick(ctx, &choices, "direction", "mail.direction", &["mail.leftToRight", "mail.rightToLeft"]),
                pick(ctx, &choices, "composerMode", "mail.composerMode", &["mail.composerPopup", "mail.composerMaximized"]),
                listed(ctx, &choices, "fontFace", "mail.fontFace", &font_faces()),
                listed(ctx, &choices, "fontSize", "mail.fontSize", &font_sizes()),
                footer_row(ctx),
            ]),
        ]);
    });
}

pub fn privacy(ctx: &Ctx) {
    ctx.load(both(ctx.service.clone()), |ctx, (settings, choices): (MailSettings, Choices)| {
        ctx.show(vec![
            section("", &ctx.say("mail.images"), "", vec![
                mail_switch(ctx, &settings, "hideRemoteImages", "settings.hideRemote", "settings.hideRemoteHint"),
                flag(ctx, &choices, "hideEmbedded", "mail.hideEmbedded", "mail.hideEmbeddedHint"),
                flag(ctx, &choices, "hideSenderImages", "mail.hideSenderImages", "mail.hideSenderImagesHint"),
                flag(ctx, &choices, "removeMetadata", "mail.removeMetadata", "mail.removeMetadataHint"),
            ]),
            section("", &ctx.say("mail.safety"), "", vec![
                mail_switch(ctx, &settings, "confirmLinks", "settings.confirmLinks", "settings.confirmLinksHint"),
                flag(ctx, &choices, "blockConfirmation", "mail.blockQuietly", "mail.blockQuietlyHint"),
                pick(ctx, &choices, "spamAction", "mail.spamAction", &["mail.spamAsk", "mail.spamOnly", "mail.spamUnsubscribe"]),
            ]),
        ]);
    });
}

/// How mail is signed and checked. It closes the keys page, which stays read-only.
pub fn pgp_section(ctx: &Ctx, choices: &Choices) -> SettingsSection {
    section("", &ctx.say("mail.pgp"), "", vec![
        flag(ctx, choices, "sign", "mail.sign", "mail.signHint"),
        flag(ctx, choices, "attachPublicKey", "mail.attachPublicKey", "mail.attachPublicKeyHint"),
        pick(ctx, choices, "pgpScheme", "mail.pgpScheme", &["mail.pgpMime", "mail.pgpInline"]),
        flag(ctx, choices, "promptPin", "mail.promptPin", "mail.promptPinHint"),
        note(&ctx.say("mail.savedOnlyKeys")),
    ])
}

fn reload_list(ctx: &Ctx) {
    let service = ctx.service.clone();
    tokio::spawn(async move { service.view_reload().await });
}

/// What is on screen already was made from the choice before.
fn chosen(ctx: &Ctx, key: &str) {
    super::redraw(ctx);
    match key {
        "pageSize" | "showMoved" | "almostAllMail" => reload_list(ctx),
        "viewMode" => {
            reload_list(ctx);
            auth::refresh_folders(&ctx.service, &ctx.app);
        }
        "folderColor" => auth::refresh_folders(&ctx.service, &ctx.app),
        _ => {}
    }
}

/// Switches, and rows that open a list of their own.
pub fn heard_choice(ctx: &Ctx, id: &str, value: i32) -> bool {
    if let Some(key) = id.strip_prefix("choice:") {
        let (service, key, position) = (ctx.service.clone(), key.to_string(), usize::try_from(value).unwrap_or(0));
        let changed = key.clone();
        ctx.act(async move { service.set_mail_choice(&key, position).await }, move |ctx| chosen(ctx, &changed));
        return true;
    }
    if let Some(key) = id.strip_prefix("list:") {
        let options = if key == "fontFace" { font_faces() } else { font_sizes() };
        let Some(app) = ctx.app.upgrade() else { return true };
        let (ctx, key) = (ctx.clone(), key.to_string());
        tray::choose(&app, &options, move |_, position| {
            let service = ctx.service.clone();
            ctx.act(async move { service.set_mail_choice(&key, position).await }, super::redraw);
        });
        return true;
    }
    false
}

pub fn identity(ctx: &Ctx) {
    let mut sections = vec![plain(vec![note(&ctx.say("settings.addressesHint"))])];
    for (position, address) in ctx.service.addresses().iter().enumerate() {
        let id = &address.id;
        let mut title = fact(&address.email, "");
        title.id = format!("default:{id}").into();
        if position == 0 {
            title.tag = ctx.say("settings.default").into();
        } else {
            title.button = ctx.say("settings.makeDefault").into();
        }
        sections.push(card(vec![
            title,
            field(&format!("name:{id}"), &ctx.say("settings.displayName"), &address.display_name),
            area(&format!("signature:{id}"), &ctx.say("settings.signature"), &address.signature_text, 3),
            button(&format!("save:{id}"), &ctx.say("common.save"), false),
        ]));
    }
    ctx.show(sections);
}

/// Saving is offered once something changed.
pub fn address_typed(ctx: &Ctx, id: &str) {
    let Some((_, address)) = id.split_once(':') else { return };
    let changed = ctx.service.addresses().iter().find(|known| known.id == address).is_some_and(|known| {
        ctx.text(&format!("name:{address}")) != known.display_name || ctx.text(&format!("signature:{address}")) != known.signature_text
    });
    ctx.enable(&format!("save:{address}"), changed);
}

fn folder_rows(ctx: &Ctx, folders: &[Folder], kind: &str) -> Vec<SettingsRow> {
    let editing = EDITING.lock().clone();
    let mut lines = Vec::new();
    if matches!(&editing, Some(Editing::New(new)) if *new == kind) {
        lines.push(field(NEW_NAME, &ctx.say("nav.name"), ""));
        lines.push(SettingsRow { second: ctx.say("common.cancel").into(), ..button(NEW_SAVE, &ctx.say("common.create"), false) });
    }
    for folder in folders.iter().filter(|folder| folder.kind == kind) {
        if let Some(Editing::Rename { id, name, .. }) = &editing
            && *id == folder.id
        {
            lines.push(field(NEW_NAME, &ctx.say("nav.rename"), name));
            lines.push(SettingsRow { second: ctx.say("common.cancel").into(), ..button(NEW_SAVE, &ctx.say("common.save"), true) });
            continue;
        }
        let mut row = entry(&format!("folder:{}", folder.id), &folder.name, "");
        row.button = ctx.say("nav.rename").into();
        row.second = ctx.say("nav.delete").into();
        lines.push(row);
    }
    lines
}

pub fn folders(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.folders().await }, |ctx, folders: Vec<Folder>| {
        ctx.show(vec![
            section("add-folder", &ctx.say("nav.folders"), &ctx.say("nav.addFolder"), folder_rows(ctx, &folders, "folder")),
            section("add-label", &ctx.say("nav.labels"), &ctx.say("nav.addLabel"), folder_rows(ctx, &folders, "label")),
        ]);
    });
}

/// The sidebar shows these too.
fn folders_changed(ctx: &Ctx) {
    *EDITING.lock() = None;
    auth::refresh_folders(&ctx.service, &ctx.app);
    folders(ctx);
}

fn save_folder(ctx: &Ctx) {
    let name = ctx.text(NEW_NAME).trim().to_string();
    let editing = EDITING.lock().clone();
    let service = ctx.service.clone();
    match editing {
        Some(Editing::New(kind)) => ctx.act(async move { service.create_label(&name, kind).await }, folders_changed),
        Some(Editing::Rename { id, color, .. }) => ctx.act(async move { service.rename_label(&id, &name, &color).await }, folders_changed),
        None => {}
    }
}

fn folder_event(ctx: &Ctx, id: &str, part: i32) {
    let service = ctx.service.clone();
    let id = id.to_string();
    if part == 0 {
        let finding = id;
        ctx.load(async move { service.folders().await }, move |ctx, known: Vec<Folder>| {
            if let Some(folder) = known.iter().find(|folder| folder.id == finding) {
                *EDITING.lock() = Some(Editing::Rename { id: finding, color: folder.color.clone().unwrap_or_default(), name: folder.name.clone() });
            }
            folders(ctx);
        });
        return;
    }
    let (delete, no) = (ctx.say("nav.delete"), ctx.say("common.cancel"));
    let title = ctx.say_with("nav.deleteTitle", &[("name", &ctx.row_title(&format!("folder:{id}")))]);
    if actions::confirmed(&title, &ctx.say("nav.deleteText"), &delete, &no) {
        ctx.act(async move { service.delete_label(&id).await }, folders_changed);
    }
}

fn forwarding_row(ctx: &Ctx, row: &Forwarding) -> SettingsRow {
    let direction = ctx.say(if row.outgoing { "forwarding.outgoing" } else { "forwarding.incoming" });
    let detail = if row.encrypted { format!("{direction} · {}", ctx.say("forwarding.encrypted")) } else { direction };
    let state = match row.state {
        ForwardingState::Pending => "forwarding.pending",
        ForwardingState::Active => "forwarding.active",
        ForwardingState::Outdated => "forwarding.outdated",
        ForwardingState::Paused => "forwarding.paused",
        ForwardingState::Rejected => "forwarding.rejected",
        ForwardingState::Unknown => "billing.otherState",
    };
    let mut line = entry(&format!("forward:{}", row.id), &row.address, &detail);
    line.tag = ctx.say(state).into();
    line.tag_good = row.state == ForwardingState::Active;
    line.tag_bad = row.state == ForwardingState::Rejected;
    line.second = ctx.say("forwarding.delete").into();
    if row.outgoing && row.state == ForwardingState::Active {
        line.button = ctx.say("forwarding.pause").into();
    } else if row.outgoing && row.state == ForwardingState::Paused {
        line.button = ctx.say("forwarding.resume").into();
    }
    line
}

/// Proton sends 0 until dates have been set.
fn reply_time(ctx: &Ctx, seconds: u64) -> String {
    if seconds == 0 { ctx.say("date.choose") } else { timing::moment_text(i64::try_from(seconds).unwrap_or(0)) }
}

fn pick_reply_time(ctx: &Ctx, id: &str) {
    let (Some(app), Some(reply)) = (ctx.app.upgrade(), take_reply(ctx)) else { return };
    *REPLY.lock() = Some(reply.clone());
    let starts = id == REPLY_FROM;
    let seconds = if starts { reply.start } else { reply.end };
    let request = moment_picker::Request {
        heading: ctx.row_title(id),
        value: if seconds == 0 { timing::tomorrow_morning() } else { timing::field_text(seconds) },
        with_time: true,
        earliest: None,
        check: None,
    };
    let ctx = ctx.clone();
    moment_picker::open(&app, &request, move |field| {
        let Ok(at) = timing::field_time(&field) else { return };
        change_reply(&ctx, |reply| if starts { reply.start = at } else { reply.end = at });
    });
}

fn reply_rows(ctx: &Ctx, reply: &AutoReply) -> Vec<SettingsRow> {
    let mut lines = vec![switch("reply-on", &ctx.say("settings.autoReplyOn"), "", reply.enabled)];
    if reply.enabled {
        let kinds = vec![ctx.say("settings.autoReplyPermanent"), ctx.say("settings.autoReplyDates")];
        lines.push(segmented("reply-kind", "", kinds, usize::from(!reply.permanent)));
        if !reply.permanent {
            lines.push(choice(REPLY_FROM, &ctx.say("settings.autoReplyFrom"), &reply_time(ctx, reply.start)));
            lines.push(choice(REPLY_TO, &ctx.say("settings.autoReplyTo"), &reply_time(ctx, reply.end)));
        }
        lines.push(field(REPLY_SUBJECT, &ctx.say("compose.subject"), &reply.subject));
        lines.push(area(REPLY_TEXT, &ctx.say("settings.autoReplyMessage"), &reply.text, 4));
    }
    lines.push(button("reply-save", &ctx.say("common.save"), true));
    lines
}

pub fn forwarding(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move {
        let rows = service.forwardings().await.unwrap_or_default();
        Ok((rows, service.auto_reply().await?))
    };
    ctx.load(task, |ctx, (forwards, reply): (Vec<Forwarding>, AutoReply)| {
        *REPLY.lock() = Some(reply.clone());
        let mut lines: Vec<SettingsRow> = forwards.iter().map(|row| forwarding_row(ctx, row)).collect();
        if lines.is_empty() {
            lines.push(note(&ctx.say("forwarding.empty")));
        }
        ctx.show(vec![
            section("", &ctx.say("forwarding.title"), "", lines),
            plain(vec![note(&ctx.say("forwarding.addHint")), ghost("manage:auto-reply", &ctx.say("tabs.openInProton"), true)]),
            section("reply", &ctx.say("settings.autoReply"), "", reply_rows(ctx, &reply)),
        ]);
    });
}

/// What the fields hold, before they are drawn again.
fn take_reply(ctx: &Ctx) -> Option<AutoReply> {
    let mut reply = REPLY.lock().clone()?;
    if ctx.has_row(REPLY_SUBJECT) {
        reply.subject = ctx.text(REPLY_SUBJECT);
        reply.text = ctx.text(REPLY_TEXT);
    }
    Some(reply)
}

fn change_reply(ctx: &Ctx, change: impl FnOnce(&mut AutoReply)) {
    let Some(mut reply) = take_reply(ctx) else { return };
    change(&mut reply);
    ctx.replace("reply", reply_rows(ctx, &reply));
    *REPLY.lock() = Some(reply);
}

fn save_reply(ctx: &Ctx) {
    let Some(reply) = take_reply(ctx) else { return };
    *REPLY.lock() = Some(reply.clone());
    let service = ctx.service.clone();
    let zone = iana_time_zone::get_timezone().unwrap_or_default();
    ctx.act(async move { service.update_auto_reply(&reply, &zone).await }, |ctx| ctx.tell(&ctx.say("settings.autoReplySaved")));
}

fn forward_event(ctx: &Ctx, id: &str, part: i32) {
    let (service, id) = (ctx.service.clone(), id.to_string());
    if part == 0 {
        let paused = ctx.row_button(&format!("forward:{id}")) == ctx.say("forwarding.pause");
        ctx.act(async move { service.pause_forwarding(&id, paused).await }, forwarding);
        return;
    }
    let title = ctx.say("forwarding.delete");
    let address = ctx.row_title(&format!("forward:{id}"));
    if actions::confirmed(&title, &ctx.say_with("forwarding.deleteText", &[("address", &address)]), &title, &ctx.say("common.cancel")) {
        ctx.act(async move { service.delete_forwarding(&id).await }, forwarding);
    }
}

pub fn heard(ctx: &Ctx, id: &str, part: i32, value: i32) {
    if heard_choice(ctx, id, value) {
        return;
    }
    let service = ctx.service.clone();
    match id {
        "nextMessageOnMove" | "autoDeleteOld" | "autoSaveContacts" | "hideRemoteImages" | "confirmLinks" => {
            let (key, on) = (id.to_string(), value == 1);
            ctx.act(async move { service.update_mail_setting(&key, on).await }, super::redraw);
        }
        "undo" => {
            let seconds = usize::try_from(value).ok().and_then(|position| UNDO_SECONDS.get(position)).copied().unwrap_or(0);
            ctx.act(async move { service.update_delay_send(seconds).await }, super::redraw);
        }
        "add-folder" | "add-label" => {
            *EDITING.lock() = Some(Editing::New(if id == "add-folder" { "folder" } else { "label" }));
            folders(ctx);
        }
        NEW_SAVE => {
            if part == 1 {
                *EDITING.lock() = None;
                folders(ctx);
            } else {
                save_folder(ctx);
            }
        }
        "reply-on" => change_reply(ctx, |reply| reply.enabled = value == 1),
        "reply-kind" => change_reply(ctx, |reply| reply.permanent = value == 0),
        "reply-save" => save_reply(ctx),
        REPLY_FROM | REPLY_TO => pick_reply_time(ctx, id),
        _ => heard_by_prefix(ctx, id, part),
    }
}

fn heard_by_prefix(ctx: &Ctx, id: &str, part: i32) {
    let service = ctx.service.clone();
    if let Some(address) = id.strip_prefix("default:") {
        let address = address.to_string();
        ctx.act(async move { service.set_default_address(&address).await }, identity);
    } else if let Some(address) = id.strip_prefix("save:") {
        let (name, text) = (ctx.text(&format!("name:{address}")), ctx.text(&format!("signature:{address}")));
        let address = address.to_string();
        ctx.act(async move { service.update_address(&address, &name, &text).await }, |ctx| {
            ctx.tell(&ctx.say("settings.addressSaved"));
            identity(ctx);
        });
    } else if let Some(folder) = id.strip_prefix("folder:") {
        folder_event(ctx, folder, part);
    } else if let Some(forward) = id.strip_prefix("forward:") {
        forward_event(ctx, forward, part);
    }
}

pub fn typed(ctx: &Ctx) {
    ctx.enable(NEW_SAVE, !ctx.text(NEW_NAME).trim().is_empty());
}
