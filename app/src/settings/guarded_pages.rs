//! What Proton guards with the account password: the ways back in, the second factor and Sentinel.
//! The person types the password into a card on the page. It is read once, sent nowhere but through
//! SRP, and the field is gone when the page is drawn again. A refusal is shown and never tried again
//! by itself: failures can lock an account. Changing the password or the keys stays on Proton's site.

use super::{
    Ctx,
    rows::{
        self, SettingsRow, button, card, entry, fact, field, first_secret, ghost, hidden_fact, hint, note, plain,
        strong, switch,
    },
};
use crate::SettingsSection;
use parking_lot::Mutex;
use proton_core::{
    Error,
    model::{Account, RecoveryPhrase, RecoveryStatus, UserChoices},
    print::qr_pixels,
    service::{Confirmation, TotpSetup},
};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use zeroize::Zeroizing;

const CONFIRM_PASSWORD: &str = "confirm-password";
const CONFIRM_CODE: &str = "confirm-code";
const CONFIRM: &str = "confirm";
const NEW_VALUE: &str = "new-value";
const TOTP_CODE: &str = "totp-code";
const QR_MODULE_PIXELS: u32 = 4;

#[derive(Clone)]
enum Action {
    Email(String),
    Phone(String),
    EmailReset(bool),
    PhoneReset(bool),
    Sentinel(bool),
    DisableTotp,
    EnableTotp(String),
    LogMode(u8),
}

#[derive(Clone)]
enum Totp {
    Scanning(TotpSetup),
    Codes(Vec<String>),
}

#[derive(Default)]
struct State {
    /// Waits for the password.
    action: Option<Action>,
    /// Which address is being typed: "email" or "phone".
    editing: Option<&'static str>,
    totp: Option<Totp>,
    has_second_factor: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(read: impl FnOnce(&mut State) -> T) -> T {
    read(STATE.lock().get_or_insert_with(State::default))
}

/// Recovery codes and a secret must not wait in memory for the next visit.
pub fn forget() {
    *STATE.lock() = None;
}

fn asked_for(ctx: &Ctx, action: &Action) -> String {
    match action {
        Action::Email(value) if value.is_empty() => ctx.say("guard.emailRemove"),
        Action::Email(value) => ctx.say_with("guard.email", &[("value", value)]),
        Action::Phone(value) if value.is_empty() => ctx.say("guard.phoneRemove"),
        Action::Phone(value) => ctx.say_with("guard.phone", &[("value", value)]),
        Action::EmailReset(on) => ctx.say(if *on { "guard.emailResetOn" } else { "guard.emailResetOff" }),
        Action::PhoneReset(on) => ctx.say(if *on { "guard.phoneResetOn" } else { "guard.phoneResetOff" }),
        Action::Sentinel(on) => ctx.say(if *on { "guard.sentinelOn" } else { "guard.sentinelOff" }),
        Action::DisableTotp => ctx.say("guard.totpOff"),
        Action::EnableTotp(_) => ctx.say("guard.totpOn"),
        Action::LogMode(_) => ctx.say("guard.logMode"),
    }
}

fn confirmation_card(ctx: &Ctx) -> Option<SettingsSection> {
    let action = with(|state| state.action.clone())?;
    let mut lines = vec![
        strong(&ctx.say("guard.title")),
        note(&asked_for(ctx, &action)),
        first_secret(CONFIRM_PASSWORD, &ctx.say("guard.password")),
    ];
    if with(|state| state.has_second_factor) {
        lines.push(rows::code(CONFIRM_CODE, &ctx.say("guard.code"), false));
    }
    lines.push(SettingsRow {
        second: ctx.say("common.cancel").into(),
        ..button(CONFIRM, &ctx.say("guard.confirm"), false)
    });
    Some(card(lines))
}

/// For a page of another file that holds something guarded. It goes last there, beside the switch.
pub fn card_for(ctx: &Ctx) -> Option<SettingsSection> {
    confirmation_card(ctx)
}

pub fn note_second_factor(second: &str) {
    with(|state| state.has_second_factor = second != "off");
}

pub fn ask_log_mode(ctx: &Ctx, mode: u8) {
    ask(ctx, Action::LogMode(mode));
}

/// Whether this was about the card.
pub fn heard_card(ctx: &Ctx, id: &str, part: i32) -> bool {
    if id != CONFIRM {
        return false;
    }
    if part == 1 {
        with(|state| state.action = None);
        super::redraw(ctx);
    } else {
        confirm(ctx);
    }
    true
}

fn with_card(ctx: &Ctx, mut sections: Vec<SettingsSection>) {
    if let Some(found) = confirmation_card(ctx) {
        sections.insert(0, found);
    }
    ctx.show(sections);
}

fn way_back(
    ctx: &Ctx,
    id: &str,
    label: &str,
    value: &str,
    reset: &str,
    on: bool,
    editing: Option<&str>,
) -> Vec<SettingsRow> {
    let shown = if value.is_empty() { ctx.say("security.recoveryNone") } else { value.to_string() };
    if editing == Some(id) {
        return vec![
            field(NEW_VALUE, &ctx.say(label), value),
            SettingsRow {
                second: ctx.say("common.cancel").into(),
                ..button(&format!("save:{id}"), &ctx.say("common.save"), true)
            },
        ];
    }
    let mut row = if value.is_empty() { fact(&ctx.say(label), &shown) } else { hidden_fact(&ctx.say(label), &shown) };
    row.id = format!("edit:{id}").into();
    row.button = ctx.say("recovery.change").into();
    let mut lines = vec![row];
    if !value.is_empty() {
        lines.push(switch(&format!("reset:{id}"), &ctx.say(reset), "", on));
    }
    lines
}

pub fn recovery(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move { Ok((service.recovery().await?, service.account().await?)) };
    ctx.load(task, |ctx, (status, account): (RecoveryStatus, Account)| {
        with(|state| state.has_second_factor = account.two_factor != "off");
        let editing = with(|state| state.editing);
        let mut lines =
            way_back(ctx, "email", "recovery.email", &status.email, "recovery.allowEmail", status.email_reset, editing);
        lines.extend(way_back(
            ctx,
            "phone",
            "recovery.phone",
            &status.phone,
            "recovery.allowPhone",
            status.phone_reset,
            editing,
        ));
        if !status.email.is_empty() || !status.phone.is_empty() {
            lines.push(hint(&ctx.say("privacy.hoverContacts")));
        }
        let phrase = match status.phrase {
            RecoveryPhrase::Off => "recovery.phraseOff",
            RecoveryPhrase::Missing => "recovery.phraseMissing",
            RecoveryPhrase::Outdated => "recovery.phraseOutdated",
            RecoveryPhrase::Set => "recovery.phraseSet",
        };
        lines.push(fact(&ctx.say("recovery.phrase"), &ctx.say(phrase)));
        lines.push(fact(
            &ctx.say("recovery.device"),
            &ctx.say(if status.device_backup { "recovery.on" } else { "recovery.off" }),
        ));
        let site = plain(vec![
            note(&ctx.say("recovery.manageHint")),
            ghost("manage:recovery", &ctx.say("tabs.openInProton"), true),
        ]);
        with_card(ctx, vec![plain(lines), site]);
    });
}

fn qr_row(uri: &str) -> Option<SettingsRow> {
    let (pixels, side) = qr_pixels(uri, QR_MODULE_PIXELS)?;
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&pixels, side, side);
    Some(rows::picture(Image::from_rgba8(buffer)))
}

fn totp_section(ctx: &Ctx, stage: &Totp) -> SettingsSection {
    let mut lines = Vec::new();
    match stage {
        Totp::Scanning(setup) => {
            lines.push(note(&ctx.say("totp.scan")));
            lines.extend(qr_row(&setup.uri));
            let mut key = entry("totp-copy", &ctx.say("totp.key"), &setup.secret);
            key.mono = true;
            key.button = ctx.say("referral.copy").into();
            lines.push(key);
            lines.push(rows::code(TOTP_CODE, &ctx.say("guard.code"), true));
            lines.push(SettingsRow {
                second: ctx.say("common.cancel").into(),
                ..button("totp-confirm", &ctx.say("guard.confirm"), false)
            });
        }
        Totp::Codes(codes) => {
            lines.push(strong(&ctx.say("totp.codes")));
            lines.extend(codes.iter().map(|code| SettingsRow { mono: true, ..entry("", "", code) }));
            lines.push(SettingsRow {
                second: ctx.say("totp.done").into(),
                ..button("totp-copy-codes", &ctx.say("referral.copy"), true)
            });
        }
    }
    card(lines)
}

pub fn protection(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move { Ok((service.account().await?, service.user_choices().await?)) };
    ctx.load(task, |ctx, (account, choices): (Account, UserChoices)| draw_protection(ctx, &account, &choices));
}

fn draw_protection(ctx: &Ctx, account: &Account, choices: &UserChoices) {
    let second = match account.two_factor {
        "totp" => "security.2faTotp",
        "fido2" => "security.2faKey",
        "both" => "security.2faBoth",
        _ => "security.2faOff",
    };
    with(|state| state.has_second_factor = account.two_factor != "off");
    let has_app = matches!(account.two_factor, "totp" | "both");
    let mut factor = fact(&ctx.say("security.2fa"), &ctx.say(second));
    factor.id = if has_app { "totp-off" } else { "totp-on" }.into();
    factor.button = ctx.say(if has_app { "totp.disable" } else { "totp.enable" }).into();
    let mut lines = vec![factor];
    if account.recovery_email.is_empty() {
        lines.push(fact(&ctx.say("security.recovery"), &ctx.say("security.recoveryNone")));
    } else {
        lines.push(hidden_fact(&ctx.say("security.recovery"), &account.recovery_email));
        lines.push(hint(&ctx.say("privacy.hoverContacts")));
    }
    if choices.sentinel_offered {
        lines.push(switch("sentinel", &ctx.say("sentinel.title"), &ctx.say("sentinel.hint"), choices.sentinel));
    }
    lines.push(note(&format!("{} account.proton.me", ctx.say("security.manage"))));
    lines.push(ghost("manage:", &ctx.say("tabs.openInProton"), true));
    let mut sections = vec![plain(lines)];
    if let Some(stage) = with(|state| state.totp.clone()) {
        sections.insert(0, totp_section(ctx, &stage));
    }
    with_card(ctx, sections);
}

async fn run(ctx: &Ctx, action: Action, confirmation: Confirmation) -> Result<Option<Totp>, Error> {
    let service = &ctx.service;
    match action {
        Action::Email(value) => service.set_recovery_email(&value, &confirmation).await?,
        Action::Phone(value) => service.set_recovery_phone(&value, &confirmation).await?,
        Action::EmailReset(on) => service.set_email_reset(on, &confirmation).await?,
        Action::PhoneReset(on) => service.set_phone_reset(on, &confirmation).await?,
        Action::Sentinel(on) => service.set_sentinel(on, &confirmation).await?,
        Action::DisableTotp => service.disable_totp(&confirmation).await?,
        Action::LogMode(mode) => service.set_log_mode(mode, &confirmation).await?,
        Action::EnableTotp(account) => {
            return Ok(Some(Totp::Scanning(service.start_totp(&confirmation, &account).await?)));
        }
    }
    Ok(None)
}

/// Reads the fields once, for the one action that waits. The page drawn after it holds no password.
fn confirm(ctx: &Ctx) {
    let Some(action) = with(|state| state.action.take()) else { return };
    let code = ctx.text(CONFIRM_CODE).trim().to_string();
    let confirmation = Confirmation {
        password: Zeroizing::new(ctx.text(CONFIRM_PASSWORD)),
        code: (!code.is_empty()).then(|| Zeroizing::new(code)),
    };
    ctx.enable(CONFIRM, false);
    let answer = ctx.clone();
    tokio::spawn(async move {
        let done = run(&answer, action, confirmation).await;
        let app = answer.app.clone();
        let _ = app.upgrade_in_event_loop(move |_| {
            match done {
                Ok(stage) => {
                    with(|state| {
                        state.editing = None;
                        state.totp = stage;
                    });
                    answer.tell(&answer.say("guard.done"));
                }
                Err(error) => answer.report(&error),
            }
            super::redraw(&answer);
        });
    });
}

fn confirm_totp(ctx: &Ctx) {
    let (service, code) = (ctx.service.clone(), ctx.text(TOTP_CODE).trim().to_string());
    ctx.load(async move { service.confirm_totp(&code).await }, |ctx, codes: Vec<String>| {
        with(|state| state.totp = Some(Totp::Codes(codes)));
        super::redraw(ctx);
    });
}

fn ask(ctx: &Ctx, action: Action) {
    with(|state| state.action = Some(action));
    super::redraw(ctx);
}

fn copy(ctx: &Ctx, text: &str) {
    if let Some(app) = ctx.app.upgrade() {
        app.invoke_copy_quietly(text.into());
        ctx.tell(&ctx.say("referral.copied"));
    }
}

fn enable_totp(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.account().await }, |ctx, account: Account| {
        ask(ctx, Action::EnableTotp(account.email));
    });
}

pub fn heard(ctx: &Ctx, id: &str, part: i32, value: i32) {
    let on = value == 1;
    match id {
        CONFIRM if part == 1 => {
            with(|state| state.action = None);
            super::redraw(ctx);
        }
        CONFIRM => confirm(ctx),
        "reset:email" => ask(ctx, Action::EmailReset(on)),
        "reset:phone" => ask(ctx, Action::PhoneReset(on)),
        "sentinel" => ask(ctx, Action::Sentinel(on)),
        "totp-off" => ask(ctx, Action::DisableTotp),
        "totp-on" => enable_totp(ctx),
        "totp-confirm" if part == 1 => {
            with(|state| state.totp = None);
            super::redraw(ctx);
        }
        "totp-confirm" => confirm_totp(ctx),
        "totp-copy" => {
            if let Some(Totp::Scanning(setup)) = with(|state| state.totp.clone()) {
                copy(ctx, &setup.secret);
            }
        }
        "totp-copy-codes" if part == 1 => {
            with(|state| state.totp = None);
            super::redraw(ctx);
        }
        "totp-copy-codes" => {
            if let Some(Totp::Codes(codes)) = with(|state| state.totp.clone()) {
                copy(ctx, &codes.join("\n"));
            }
        }
        _ => heard_address(ctx, id, part),
    }
}

fn heard_address(ctx: &Ctx, id: &str, part: i32) {
    if let Some(kind) = id.strip_prefix("edit:") {
        with(|state| state.editing = if kind == "email" { Some("email") } else { Some("phone") });
        super::redraw(ctx);
    } else if let Some(kind) = id.strip_prefix("save:") {
        if part == 1 {
            with(|state| state.editing = None);
            super::redraw(ctx);
            return;
        }
        let value = ctx.text(NEW_VALUE).trim().to_string();
        ask(ctx, if kind == "email" { Action::Email(value) } else { Action::Phone(value) });
    }
}

/// A whole code goes by itself, as at sign-in. A password does not: the person says when.
pub fn typed(ctx: &Ctx, id: &str) {
    if id == TOTP_CODE {
        let digits = ctx.tidy_code(TOTP_CODE);
        ctx.enable("totp-confirm", rows::is_whole_code(&digits));
        if rows::is_whole_code(&digits) {
            confirm_totp(ctx);
        }
        return;
    }
    let digits = if ctx.has_row(CONFIRM_CODE) { ctx.tidy_code(CONFIRM_CODE) } else { String::new() };
    let ready = !ctx.text(CONFIRM_PASSWORD).is_empty() && (!ctx.has_row(CONFIRM_CODE) || rows::is_whole_code(&digits));
    ctx.enable(CONFIRM, ready);
    if ready && id == CONFIRM_CODE {
        confirm(ctx);
    }
}
