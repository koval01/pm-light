//! What Proton knows about the account: the account, subscription, recovery, security, sessions, keys and domains.
//! What changes keys, money or the way back into an account happens on Proton's own site.

use super::{
    Ctx, guarded_pages, lock_page,
    mail_pages,
    rows::{self, area, button, entry, fact, ghost, hidden_fact, hint, meter, note, plain, section, segmented, switch},
};
use crate::{SettingsRow, SettingsSection, actions, auth};
use parking_lot::Mutex;
use proton_core::{
    domain_check::LiveCheck,
    mapping::NEWS,
    model::{Account, AccountSession, AuthLogEntry, Billing, InvoiceState, Storage, UserChoices},
};
use std::collections::HashMap;

static CHECKS: Mutex<Option<HashMap<String, LiveCheck>>> = Mutex::new(None);

fn storage_row(ctx: &Ctx, storage: &Storage) -> SettingsRow {
    meter(storage.used, storage.max, &storage_text(ctx, storage))
}

fn manage(ctx: &Ctx, slug: &str, text: &str) -> SettingsSection {
    plain(vec![note(text), ghost(&format!("manage:{slug}"), &ctx.say("tabs.openInProton"), true)])
}

/// The plan, and when it renews or ends.
pub fn plan_text(ctx: &Ctx, account: &Account) -> String {
    account.plan.as_ref().map_or_else(
        || ctx.say(if account.paid { "account.planPaid" } else { "account.planFree" }),
        |plan| {
            let key = if plan.renews { "account.renews" } else { "account.ends" };
            format!("{} · {}", plan.title, ctx.say_with(key, &[("date", &rows::day(plan.period_end))]))
        },
    )
}

/// The space used, out of the space there is.
pub fn storage_text(ctx: &Ctx, storage: &Storage) -> String {
    ctx.say_with("settings.storage", &[("used", &rows::size(ctx, storage.used)), ("max", &rows::size(ctx, storage.max))])
}

pub fn overview(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move {
        let account = service.account().await?;
        Ok((account, service.storage().await.ok()))
    };
    ctx.load(task, |ctx, (account, storage): (Account, Option<Storage>)| {
        let plan = plan_text(ctx, &account);
        let mut lines = vec![hidden_fact(&account.name, &account.email), fact(&ctx.say("account.plan"), &plan)];
        lines.extend(storage.iter().map(|storage| storage_row(ctx, storage)));
        lines.push(ghost("sign-out", &ctx.say("settings.signOut"), true));
        ctx.show(vec![plain(lines)]);
    });
}

fn plan_section(ctx: &Ctx, billing: &Billing, storage: Option<&Storage>) -> SettingsSection {
    let mut lines = Vec::new();
    match &billing.plan {
        Some(plan) => {
            lines.push(fact(&plan.title, &ctx.say_with("billing.months", &[("count", &plan.months.to_string())])));
            lines.push(fact(&ctx.say("billing.price"), &rows::money(plan.amount, &plan.currency)));
            let date = rows::day(plan.period_end);
            let value = if plan.renews { format!("{date} · {}", rows::money(plan.renew_amount, &plan.currency)) } else { date };
            lines.push(fact(&ctx.say(if plan.renews { "billing.renews" } else { "billing.ends" }), &value));
        }
        None => lines.push(note(&ctx.say("account.planFree"))),
    }
    lines.extend(storage.map(|storage| storage_row(ctx, storage)));
    section("", &ctx.say("account.plan"), "", lines)
}

pub fn subscription(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move {
        let billing = service.billing().await?;
        Ok((billing, service.storage().await.ok(), service.user_choices().await?))
    };
    ctx.load(task, |ctx, (billing, storage, choices): (Billing, Option<Storage>, UserChoices)| {
        let mut sections = vec![plan_section(ctx, &billing, storage.as_ref())];
        if !billing.methods.is_empty() {
            let mut lines: Vec<SettingsRow> = billing
                .methods
                .iter()
                .map(|method| SettingsRow {
                    tag: if method.is_default { ctx.say("billing.default").into() } else { "".into() },
                    hidden_detail: true,
                    ..entry("", "", &method.label)
                })
                .collect();
            lines.push(hint(&ctx.say("privacy.hoverPayment")));
            sections.push(section("", &ctx.say("billing.methods"), "", lines));
        }
        sections.push(section("", &ctx.say("billing.credit"), "", vec![fact(&ctx.say("billing.creditAvailable"), &rows::money(billing.credit, &billing.currency))]));
        if !billing.invoices.is_empty() {
            let lines = billing
                .invoices
                .iter()
                .map(|invoice| {
                    let state = match invoice.state {
                        InvoiceState::Paid => "billing.paid",
                        InvoiceState::Unpaid => "billing.unpaid",
                        InvoiceState::Other => "billing.otherState",
                    };
                    SettingsRow {
                        tag: ctx.say(state).into(),
                        tag_good: invoice.state == InvoiceState::Paid,
                        tag_bad: invoice.state == InvoiceState::Unpaid,
                        value: rows::money(invoice.amount, &invoice.currency).into(),
                        ..entry("", &rows::day(invoice.created_at), "")
                    }
                })
                .collect();
            sections.push(section("", &ctx.say("billing.invoices"), "", lines));
        }
        sections.push(section("", &ctx.say("billing.invoiceDetails"), "", vec![
            area("invoice-text", "", &choices.invoice_text, 3),
            button("invoice-save", &ctx.say("common.save"), true),
        ]));
        sections.push(news_section(ctx, &choices));
        sections.push(manage(ctx, "subscription", &ctx.say("billing.manageHint")));
        ctx.show(sections);
    });
}

fn session_row(ctx: &Ctx, session: &AccountSession) -> SettingsRow {
    let name = if session.current { "Proton Mail Light" } else { &session.client };
    let mut row = entry(&format!("revoke:{}", session.id), name, &rows::moment(session.created_at));
    if session.current {
        row.tag = ctx.say("sessions.current").into();
    } else if session.revocable {
        row.button = ctx.say("sessions.revoke").into();
    }
    row
}

/// The events people meet most. The rest show the server's own description.
fn log_label(ctx: &Ctx, entry: &AuthLogEntry) -> String {
    let key = match entry.event {
        0 => "log.failedPassword",
        1 => "log.signedIn",
        2 => "log.signedOut",
        3 => "log.failed2fa",
        4 => "log.awaiting2fa",
        9 => "log.blocked",
        70 => "log.revokedAll",
        71 => "log.revokedOne",
        _ => return entry.description.clone(),
    };
    ctx.say(key)
}

fn log_row(ctx: &Ctx, line: &AuthLogEntry) -> SettingsRow {
    let place: Vec<&str> = [line.ip.as_str(), line.location.as_str(), line.device.as_str()].into_iter().filter(|text| !text.is_empty()).collect();
    SettingsRow {
        bad: line.failed,
        hidden: true,
        value: place.join(" · ").into(),
        ..entry("", &log_label(ctx, line), &rows::moment(line.time))
    }
}

fn log_section(ctx: &Ctx, mode: u64, entries: &[AuthLogEntry]) -> SettingsSection {
    let modes = ["security.logOff", "security.logBasic", "security.logAdvanced"].map(|key| ctx.say(key)).to_vec();
    let mut lines = vec![segmented("log-mode", "", modes, usize::try_from(mode).unwrap_or(0))];
    if mode > 0 && !entries.is_empty() {
        let mut head = fact(&ctx.say_with("security.logEntries", &[("count", &entries.len().to_string())]), "");
        head.id = "log-clear".into();
        head.button = ctx.say("security.logClear").into();
        lines.push(head);
        lines.extend(entries.iter().map(|line| log_row(ctx, line)));
    }
    section("", &ctx.say("security.log"), "", lines)
}

pub fn security(ctx: &Ctx) {
    let service = ctx.service.clone();
    let task = async move {
        let sessions = service.sessions().await?;
        let account = service.account().await?;
        let log = if account.log_mode > 0 { service.auth_log().await.unwrap_or_default() } else { Vec::new() };
        Ok((sessions, account.log_mode, log, service.user_choices().await?, account.two_factor))
    };
    ctx.load(task, |ctx, (sessions, mode, log, choices, second): (Vec<AccountSession>, u64, Vec<AuthLogEntry>, UserChoices, &'static str)| {
        guarded_pages::note_second_factor(second);
        let others = if sessions.len() > 1 { ctx.say("sessions.revokeOthers") } else { String::new() };
        let lines = sessions.iter().map(|session| session_row(ctx, session)).collect();
        let mut sections = vec![lock_page::section_of(ctx), section("revoke-others", &ctx.say("sessions.title"), &others, lines), log_section(ctx, mode, &log), privacy_section(ctx, &choices)];
        sections.extend(guarded_pages::card_for(ctx));
        ctx.show(sections);
    });
}

/// Switches of the account, wherever they stand.
pub fn heard_account(ctx: &Ctx, id: &str, value: i32) -> bool {
    let (service, on) = (ctx.service.clone(), value == 1);
    if let Some(key) = id.strip_prefix("flag:") {
        let key = key.to_string();
        ctx.act(async move { service.set_user_flag(&key, on).await }, super::redraw);
    } else if let Some(name) = id.strip_prefix("news:") {
        let name = name.to_string();
        ctx.act(async move { service.set_news(&name, on).await }, super::redraw);
    } else if id == "breach" {
        ctx.act(async move { service.set_breach_alerts(on).await }, super::redraw);
    } else if id == "invoice-save" {
        let text = ctx.text("invoice-text");
        ctx.act(async move { service.set_invoice_text(&text).await }, |ctx| ctx.tell(&ctx.say("billing.invoiceSaved")));
    } else {
        return false;
    }
    true
}

pub fn heard_security(ctx: &Ctx, id: &str, value: i32) {
    if heard_account(ctx, id, value) {
        return;
    }
    let service = ctx.service.clone();
    if let Some(session) = id.strip_prefix("revoke:") {
        let session = session.to_string();
        ctx.act(async move { service.revoke_session(&session).await }, super::redraw);
    } else if id == "revoke-others" {
        let title = ctx.say("sessions.revokeOthers");
        if actions::confirmed(&title, &ctx.say("sessions.revokeOthersText"), &ctx.say("sessions.revoke"), &ctx.say("common.cancel")) {
            ctx.act(async move { service.revoke_other_sessions().await }, super::redraw);
        }
    } else if id == "log-mode" {
        guarded_pages::ask_log_mode(ctx, u8::try_from(value).unwrap_or(0));
    } else if id == "log-clear" {
        let title = ctx.say("security.logClear");
        if actions::confirmed(&title, &ctx.say("security.logClearText"), &title, &ctx.say("common.cancel")) {
            ctx.act(async move { service.clear_auth_log().await }, super::redraw);
        }
    }
}

const NEWS_WORDS: [(&str, &str); 13] = [
    ("Announcements", "news.announcements"),
    ("Newsletter", "news.newsletter"),
    ("Beta", "news.beta"),
    ("Business", "news.business"),
    ("Offers", "news.offers"),
    ("NewEmailNotif", "news.newMail"),
    ("Onboarding", "news.onboarding"),
    ("UserSurveys", "news.surveys"),
    ("InboxNews", "news.inbox"),
    ("VpnNews", "news.vpn"),
    ("DriveNews", "news.drive"),
    ("PassNews", "news.pass"),
    ("AccountRecovery", "news.recovery"),
];

fn news_section(ctx: &Ctx, choices: &UserChoices) -> SettingsSection {
    let lines = NEWS_WORDS.iter().filter_map(|(name, label)| {
        let bit = NEWS.iter().find(|(known, _)| known == name)?.1;
        Some(switch(&format!("news:{name}"), &ctx.say(label), "", choices.news & (1 << bit) != 0))
    });
    section("", &ctx.say("news.title"), "", lines.collect())
}

fn privacy_section(ctx: &Ctx, choices: &UserChoices) -> SettingsSection {
    let mut lines = vec![
        switch("flag:telemetry", &ctx.say("privacy.telemetry"), &ctx.say("privacy.telemetryHint"), choices.telemetry),
        switch("flag:crashReports", &ctx.say("privacy.crashReports"), &ctx.say("privacy.crashReportsHint"), choices.crash_reports),
    ];
    if choices.breach_alerts_offered {
        lines.push(switch("breach", &ctx.say("privacy.breachAlerts"), &ctx.say("privacy.breachAlertsHint"), choices.breach_alerts));
    }
    section("", &ctx.say("privacy.title"), "", lines)
}

pub fn encryption(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.mail_choices().await }, |ctx, choices| draw_encryption(ctx, &choices));
}

fn draw_encryption(ctx: &Ctx, choices: &mail_pages::Choices) {
    let mut lines = vec![note(&ctx.say("keys.about"))];
    lines.extend(ctx.service.keys().iter().map(|key| {
        let name = if key.address.is_empty() { ctx.say("keys.account") } else { key.address.clone() };
        SettingsRow {
            mono: true,
            tag: if key.primary { ctx.say("keys.primary").into() } else { "".into() },
            ..entry("", &name, &key.fingerprint)
        }
    }));
    ctx.show(vec![plain(lines), mail_pages::pgp_section(ctx, choices), manage(ctx, "encryption-keys", &ctx.say("keys.manageHint"))]);
}

fn live_rows(ctx: &Ctx, check: &LiveCheck) -> Vec<SettingsRow> {
    [
        ("domains.verified", &check.verified),
        ("domains.mx", &check.mx),
        ("domains.spf", &check.spf),
        ("domains.dkim", &check.dkim),
        ("domains.dmarc", &check.dmarc),
    ]
    .into_iter()
    .map(|(label, finding)| {
        let found = if finding.found.is_empty() { ctx.say("domains.nothing") } else { finding.found.join(", ") };
        SettingsRow {
            mono: true,
            tag: ctx.say(if finding.ok { "domains.correct" } else { "domains.wrong" }).into(),
            tag_bad: !finding.ok,
            tag_good: finding.ok,
            ..entry("", &ctx.say(label), &found)
        }
    })
    .collect()
}

pub fn domains(ctx: &Ctx) {
    let service = ctx.service.clone();
    ctx.load(async move { service.domains().await }, |ctx, domains| {
        let mut lines = Vec::new();
        for domain in &domains {
            let mut head = entry(&format!("check:{}", domain.name), &domain.name, "");
            head.button = ctx.say("domains.checkNow").into();
            lines.push(head);
            let states = [domain.verified, domain.mx, domain.spf, domain.dkim, domain.dmarc];
            let labels = ["domains.verified", "domains.mx", "domains.spf", "domains.dkim", "domains.dmarc"].map(|key| ctx.say(key));
            lines.push(rows::tags(labels.to_vec(), &states));
            if let Some(check) = CHECKS.lock().as_ref().and_then(|checks| checks.get(&domain.name)) {
                lines.extend(live_rows(ctx, check));
            }
        }
        if domains.is_empty() {
            lines.push(note(&ctx.say("domains.empty")));
        }
        ctx.show(vec![plain(lines), manage(ctx, "domain-names", &ctx.say("domains.manageHint"))]);
    });
}

pub fn heard(ctx: &Ctx, id: &str, _part: i32) {
    if id == "sign-out" {
        if let Some(app) = ctx.app.upgrade() {
            auth::ask_to_sign_out(&ctx.service, &app);
        }
    } else if let Some(name) = id.strip_prefix("check:") {
        let (service, name) = (ctx.service.clone(), name.to_string());
        ctx.tell(&ctx.say("domains.checking"));
        ctx.load(async move { service.check_domain(&name).await }, |ctx, check: LiveCheck| {
            CHECKS.lock().get_or_insert_with(HashMap::new).insert(check.name.clone(), check);
            domains(ctx);
        });
    }
}
