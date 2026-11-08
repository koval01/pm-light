//! What an account has besides its mail, which Proton shows on its own site: invitations, its other
//! apps, SMTP tokens, the users of an organization. Only starting an import stays there, because
//! it signs in at Google or Microsoft in a browser.

use super::{
    Ctx,
    rows::{self, SettingsRow, button, choice, entry, fact, field, ghost, hint, meter, note, plain, section, strong},
};
use crate::{actions, tray, view};
use parking_lot::Mutex;
use proton_core::model::{Member, MemberState, Organization, Referrals, ReferredState, SmtpToken};

const ANDROID: &str = "https://play.google.com/store/apps/details?id=ch.protonmail.android";
const IOS: &str = "https://apps.apple.com/app/apple-store/id979659905";
const DESKTOP: &str = "https://proton.me/mail/download";
const BRIDGE: &str = "https://proton.me/mail/bridge";
const EXPORT_TOOL: &str = "https://proton.me/support/proton-mail-export-tool";
const SMTP_SERVER: &str = "smtp.protonmail.ch";
const SMTP_PORT: &str = "587";
const TOKEN_NAME: &str = "token-name";
const TOKEN_ADDRESS: &str = "token-address";
const TOKEN_CREATE: &str = "token-create";

/// What the page holds on to while it is open. The link and a new token are read back from here
/// to be copied, and a new token goes when the page does.
#[derive(Default)]
struct State {
    link: String,
    address: usize,
    new_token: Option<(String, String)>,
    tokens: Vec<SmtpToken>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(read: impl FnOnce(&mut State) -> T) -> T {
    read(STATE.lock().get_or_insert_with(State::default))
}

fn money(cents: u64, currency: &str) -> String {
    rows::money(i64::try_from(cents).unwrap_or(0), currency)
}

const fn referred_state(state: ReferredState) -> &'static str {
    match state {
        ReferredState::Invited => "referral.stateInvited",
        ReferredState::SignedUp => "referral.stateSignedUp",
        ReferredState::Trial => "referral.stateTrial",
        ReferredState::Completed => "referral.stateCompleted",
        ReferredState::Rewarded => "referral.stateRewarded",
    }
}

fn draw_referrals(ctx: &Ctx, referrals: &Referrals) {
    with(|state| state.link.clone_from(&referrals.link));
    let mut top = vec![note(&ctx.say("referral.about"))];
    if referrals.most > 0 {
        let currency = &referrals.currency;
        top.push(note(&ctx.say_with("referral.reward", &[
            ("you", &money(referrals.for_you, currency)),
            ("friend", &money(referrals.for_friend, currency)),
            ("most", &money(referrals.most, currency)),
        ])));
    }
    if referrals.link.is_empty() {
        top.push(note(&ctx.say("referral.unavailable")));
    } else {
        let mut link = entry("referral-copy", &ctx.say("referral.link"), &referrals.link);
        link.button = ctx.say("referral.copy").into();
        top.push(link);
    }
    let earned = ctx.say_with("referral.earnedValue", &[("months", &referrals.reward_months.to_string()), ("limit", &referrals.reward_limit.to_string())]);
    top.push(fact(&ctx.say("referral.earned"), &earned));
    top.push(fact(&ctx.say("referral.left"), &referrals.emails_left.to_string()));
    let mut people: Vec<SettingsRow> = referrals
        .people
        .iter()
        .map(|person| {
            let name = if person.email.is_empty() { ctx.say("referral.someone") } else { person.email.clone() };
            let mut row = entry("", &name, &rows::day(person.created_at));
            row.tag = ctx.say(referred_state(person.state)).into();
            row.tag_good = matches!(person.state, ReferredState::Completed | ReferredState::Rewarded);
            row
        })
        .collect();
    if people.is_empty() {
        people.push(note(&ctx.say("referral.none")));
    }
    ctx.show(vec![plain(top), section("", &ctx.say("referral.people"), "", people)]);
}

pub fn referral(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.referrals().await }, |ctx, referrals: Referrals| draw_referrals(ctx, &referrals));
}

fn app_row(ctx: &Ctx, id: &str, title: &str, detail: &str) -> SettingsRow {
    let mut row = entry(id, &ctx.say(title), &if detail.is_empty() { String::new() } else { ctx.say(detail) });
    row.button = ctx.say("apps.open").into();
    row
}

pub fn apps(ctx: &Ctx) {
    ctx.show(vec![
        section("", &ctx.say("apps.mobile"), "", vec![app_row(ctx, "open-android", "apps.android", ""), app_row(ctx, "open-ios", "apps.ios", "")]),
        section("", &ctx.say("apps.computer"), "", vec![app_row(ctx, "open-desktop", "apps.desktop", ""), app_row(ctx, "open-bridge", "apps.bridge", "apps.bridgeAbout")]),
    ]);
}

pub fn backup(ctx: &Ctx) {
    ctx.show(vec![plain(vec![note(&ctx.say("backup.about")), button("open-export", &ctx.say("backup.get"), true)])]);
}

fn token_row(ctx: &Ctx, token: &SmtpToken) -> SettingsRow {
    let address = ctx.service.addresses().into_iter().find(|address| address.id == token.address_id).map_or_else(String::new, |address| address.email);
    let used = if token.last_used == 0 { ctx.say("smtp.neverUsed") } else { ctx.say_with("smtp.used", &[("when", &rows::moment(token.last_used))]) };
    let mut row = entry(&format!("token:{}", token.id), &token.name, &format!("{address} · {used}"));
    row.button = ctx.say("smtp.delete").into();
    row
}

fn new_token_rows(ctx: &Ctx, name: &str, code: &str) -> Vec<SettingsRow> {
    let copyable = |id: &str, title: &str, value: &str| {
        let mut row = entry(id, &ctx.say(title), value);
        row.mono = true;
        row.button = ctx.say("referral.copy").into();
        row
    };
    vec![
        strong(&ctx.say("smtp.once")),
        copyable("copy-user", "smtp.user", name),
        copyable("copy-token", "smtp.token", code),
        copyable("copy-server", "smtp.server", SMTP_SERVER),
        copyable("copy-port", "smtp.port", SMTP_PORT),
        ghost("token-done", &ctx.say("smtp.done"), true),
    ]
}

fn draw_imap(ctx: &Ctx, tokens: Vec<SmtpToken>, has_domain: bool) {
    let bridge = section("", &ctx.say("smtp.bridgeTitle"), "", vec![note(&ctx.say("smtp.bridgeAbout")), button("open-bridge", &ctx.say("apps.bridge"), true)]);
    let mut lines = vec![note(&ctx.say("smtp.about"))];
    if let Some((name, code)) = with(|state| state.new_token.clone()) {
        lines.extend(new_token_rows(ctx, &name, &code));
    } else if has_domain {
        let addresses = ctx.service.addresses();
        let chosen = with(|state| state.address);
        let address = addresses.get(chosen).map_or_else(String::new, |address| address.email.clone());
        lines.push(field(TOKEN_NAME, &ctx.say("smtp.name"), ""));
        lines.push(choice(TOKEN_ADDRESS, &ctx.say("smtp.address"), &address));
        lines.push(button(TOKEN_CREATE, &ctx.say("smtp.create"), false));
    } else {
        lines.push(hint(&ctx.say("smtp.noDomain")));
    }
    if tokens.is_empty() {
        lines.push(note(&ctx.say("smtp.none")));
    }
    lines.extend(tokens.iter().map(|token| token_row(ctx, token)));
    with(|state| state.tokens = tokens);
    ctx.show(vec![bridge, section("", &ctx.say("smtp.title"), "", lines)]);
}

pub fn imap(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move {
        // A token is made for an address on one's own domain: without a domain Proton refuses the
        // list, and there is nothing in it to show.
        let has_domain = !service.domains().await.unwrap_or_default().is_empty();
        let tokens = if has_domain { service.smtp_tokens().await.unwrap_or_default() } else { Vec::new() };
        Ok((tokens, has_domain))
    };
    ctx.load(task, |ctx, (tokens, has_domain): (Vec<SmtpToken>, bool)| draw_imap(ctx, tokens, has_domain));
}

fn create_token(ctx: &Ctx) {
    let name = ctx.text(TOKEN_NAME).trim().to_string();
    let addresses = ctx.service.addresses();
    let Some(address) = addresses.get(with(|state| state.address)).cloned() else { return };
    let service = ctx.service.clone();
    ctx.load(async move { service.create_smtp_token(&address.id, &name).await.map(|code| (address.email, code)) }, |ctx, made: (String, String)| {
        with(|state| state.new_token = Some(made));
        imap(ctx);
    });
}

fn choose_address(ctx: &Ctx) {
    let Some(app) = ctx.app.upgrade() else { return };
    let options: Vec<String> = ctx.service.addresses().into_iter().map(|address| address.email).collect();
    let ctx = ctx.clone();
    tray::choose(&app, &options, move |_, chosen| {
        with(|state| state.address = chosen);
        imap(&ctx);
    });
}

fn delete_token(ctx: &Ctx, id: &str) {
    let name = with(|state| state.tokens.iter().find(|token| token.id == id).map(|token| token.name.clone())).unwrap_or_default();
    let title = ctx.say_with("smtp.deleteTitle", &[("name", &name)]);
    if actions::confirmed(&title, &ctx.say("smtp.deleteText"), &ctx.say("smtp.delete"), &ctx.say("common.cancel")) {
        let (service, id) = (ctx.service.clone(), id.to_string());
        ctx.act(async move { service.delete_smtp_token(&id).await }, imap);
    }
}

fn member_row(ctx: &Ctx, member: &Member) -> SettingsRow {
    let space = ctx.say_with("org.of", &[("used", &rows::size(ctx, member.used_space)), ("max", &rows::size(ctx, member.max_space))]);
    let mut row = entry("", &member.name, &space);
    row.value = ctx.say(if member.admin { "org.admin" } else { "org.member" }).into();
    match member.state {
        MemberState::Disabled => {
            row.tag = ctx.say("org.disabled").into();
            row.tag_bad = true;
        }
        MemberState::Invited => row.tag = ctx.say("org.invited").into(),
        MemberState::Enabled if member.own => {
            row.tag = ctx.say("org.you").into();
            row.tag_good = true;
        }
        MemberState::Enabled => {}
    }
    row
}

pub fn organization(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.organization().await }, |ctx, organization: Organization| {
        let of = |(used, max): (u64, u64)| ctx.say_with("org.of", &[("used", &used.to_string()), ("max", &max.to_string())]);
        let manage = plain(vec![note(&ctx.say("org.manageHint")), ghost("manage:multi-user-support", &ctx.say("tabs.openInProton"), true)]);
        if organization.people.len() < 2 && organization.members.1 < 2 {
            ctx.show(vec![plain(vec![note(&ctx.say("org.none"))]), manage]);
            return;
        }
        let (used, max) = organization.space;
        let space = ctx.say_with("org.of", &[("used", &rows::size(ctx, used)), ("max", &rows::size(ctx, max))]);
        let mut top = Vec::new();
        if !organization.name.is_empty() {
            top.push(strong(&organization.name));
        }
        top.push(fact(&ctx.say("org.seats"), &of(organization.members)));
        top.push(fact(&ctx.say("org.addresses"), &of(organization.addresses)));
        top.push(fact(&ctx.say("org.domains"), &of(organization.domains)));
        top.push(meter(used, max, &space));
        let people = organization.people.iter().map(|member| member_row(ctx, member)).collect();
        ctx.show(vec![plain(top), section("", &ctx.say("org.users"), "", people), manage]);
    });
}

fn copy(ctx: &Ctx, text: &str) {
    if let Some(app) = ctx.app.upgrade() {
        app.invoke_copy_quietly(text.into());
        ctx.tell(&ctx.say("referral.copied"));
    }
}

pub fn heard(ctx: &Ctx, id: &str) {
    match id {
        "open-android" => view::open_link(ANDROID),
        "open-ios" => view::open_link(IOS),
        "open-desktop" => view::open_link(DESKTOP),
        "open-bridge" => view::open_link(BRIDGE),
        "open-export" => view::open_link(EXPORT_TOOL),
        "referral-copy" => copy(ctx, &with(|state| state.link.clone())),
        "copy-user" => copy(ctx, &with(|state| state.new_token.clone()).map_or_else(String::new, |(name, _)| name)),
        "copy-token" => copy(ctx, &with(|state| state.new_token.clone()).map_or_else(String::new, |(_, code)| code)),
        "copy-server" => copy(ctx, SMTP_SERVER),
        "copy-port" => copy(ctx, SMTP_PORT),
        "token-done" => {
            with(|state| state.new_token = None);
            imap(ctx);
        }
        TOKEN_ADDRESS => choose_address(ctx),
        TOKEN_CREATE => create_token(ctx),
        _ => {
            if let Some(token) = id.strip_prefix("token:") {
                delete_token(ctx, token);
            }
        }
    }
}

pub fn typed(ctx: &Ctx) {
    ctx.enable(TOKEN_CREATE, !ctx.text(TOKEN_NAME).trim().is_empty());
}

/// A token shown once must not wait in memory for the next visit.
pub fn forget() {
    *STATE.lock() = None;
}
