//! Tor's own tab: whether it is used and by which route, how it stands, the circuit to Proton drawn from the person
//! to Proton's server, and new circuits. The connection tab shows a line of it and leads here.

use super::{
    Ctx,
    rows::{self, ghost, note, plain, section, segmented, switch},
};
use crate::{App, actions, guard, preferences, tor_link};

pub fn page(ctx: &Ctx) {
    let Some(app) = ctx.app.upgrade() else { return };
    ctx.show(vec![
        plain(vec![
            note(&ctx.say("connection.torAbout")),
            ghost("open-connection", &ctx.say("tor.toConnection"), true),
        ]),
        section("tor", "", "", rows_of(ctx, &app)),
    ]);
}

/// What the connection tab shows of Tor: how it stands, and the way here.
pub fn summary(ctx: &Ctx, app: &App) -> Vec<crate::SettingsRow> {
    let on = preferences::get().tor.is_some_and(|choice| choice.on);
    let state = if !on {
        ctx.say("tor.off")
    } else if app.get_tor_text().is_empty() {
        ctx.say("tor.failed")
    } else {
        app.get_tor_text().to_string()
    };
    vec![rows::fact(&ctx.say("connection.torState"), &state), ghost("open-tor", &ctx.say("connection.toTor"), true)]
}

/// The Tor part of a page drawn again, for a new state or circuit. The rest of the page is left alone.
pub fn redraw(ctx: &Ctx, app: &App, tab: &str) {
    let rows = if tab == "tor" { rows_of(ctx, app) } else { summary(ctx, app) };
    ctx.replace("tor", rows);
}

pub fn heard(ctx: &Ctx, id: &str, value: i32) {
    match id {
        "tor" => change_tor(ctx, |choice| choice.on = value == 1),
        "tor-route" => change_tor(ctx, |choice| {
            choice.route = if value == 1 { proton_core::tor::Route::Exit } else { proton_core::tor::Route::Onion };
        }),
        "tor-new" => renew_circuits(ctx),
        "open-connection" => super::open_tab(ctx, "connection"),
        _ => {}
    }
}

/// Tor: whether it is used, by which route, how it stands, and new circuits. A change of either of the first two
/// starts the app again, so that no request of the old way outlives the change.
fn rows_of(ctx: &Ctx, app: &App) -> Vec<crate::SettingsRow> {
    let choice = preferences::get().tor.unwrap_or_default();
    let routes = vec![ctx.say("connection.torOnion"), ctx.say("connection.torExit")];
    let route = usize::from(choice.route == proton_core::tor::Route::Exit);
    let mut lines = vec![
        switch("tor", &ctx.say("connection.tor"), &ctx.say("connection.torHint"), choice.on),
        segmented("tor-route", &ctx.say("connection.torRoute"), routes, route),
    ];
    if choice.on {
        let state = if app.get_tor_text().is_empty() { ctx.say("tor.failed") } else { app.get_tor_text().to_string() };
        lines.push(rows::fact(&ctx.say("connection.torState"), &state));
        lines.extend(circuit_rows(ctx, choice.route));
        let can_renew = tor_link::tor().is_some() && !tor_link::renewing();
        lines.push(rows::button("tor-new", &ctx.say("connection.torNew"), can_renew));
    }
    lines
}

/// The circuit to Proton as a line from the person down to Proton, each relay with what it sees, its country and
/// its address, as Tor Browser shows them. Only once Proton has been reached: before that there is no circuit.
fn circuit_rows(ctx: &Ctx, route: proton_core::tor::Route) -> Vec<crate::SettingsRow> {
    use rows::{Rail, hop};
    let hops = tor_link::tor().map_or_default(|tor| tor.status().hops);
    let Some(last) = hops.len().checked_sub(1) else { return Vec::new() };
    let onion = route == proton_core::tor::Route::Onion;
    let mut lines = vec![hop(
        &ctx.say("connection.torYou"),
        &ctx.say("connection.torYouDetail"),
        "",
        "",
        Rail { first: true, ..Rail::default() },
        true,
    )];
    for (index, relay) in hops.iter().enumerate() {
        let (title, detail) = match index {
            0 => ("connection.torEntry", "connection.torEntryDetail"),
            _ if index == last && onion => ("connection.torMeeting", "connection.torMeetingDetail"),
            _ if index == last => ("connection.torExitRelay", "connection.torExitDetail"),
            _ => ("connection.torMiddle", "connection.torMiddleDetail"),
        };
        let rail = Rail { hidden_below: index == last && onion, ..Rail::default() };
        lines.push(hop(&ctx.say(title), &ctx.say(detail), &tor_link::hop_text(relay), "", rail, false));
    }
    if onion {
        let rail = Rail { hidden_above: true, hidden_below: true, ..Rail::default() };
        lines.push(hop(
            &ctx.say("connection.torProtonRelays"),
            &ctx.say("connection.torProtonRelaysDetail"),
            "",
            "",
            rail,
            false,
        ));
    }
    let server = route.api().trim_start_matches("https://");
    let (detail, rail) = if onion {
        ("", Rail { last: true, hidden_above: true, ..Rail::default() })
    } else {
        ("connection.torProtonExit", Rail { last: true, ..Rail::default() })
    };
    let detail = if detail.is_empty() { String::new() } else { ctx.say(detail) };
    lines.push(hop(&ctx.say("connection.torProton"), &detail, "", server, rail, true));
    lines
}

/// New circuits, with the button greyed out until Proton has been reached over them: a second press before then
/// would give up a circuit nobody has seen. The rows are drawn again with the new relays either way.
fn renew_circuits(ctx: &Ctx) {
    ctx.enable("tor-new", false);
    let (service, ctx) = (ctx.service.clone(), ctx.clone());
    tokio::spawn(async move {
        let reached = tor_link::new_circuits(service).await;
        let _ = ctx.app.clone().upgrade_in_event_loop(move |app| {
            if ctx.has_row("tor-new") {
                ctx.replace("tor", rows_of(&ctx, &app));
            }
            match reached {
                Ok(()) => ctx.tell(&ctx.say("connection.torNewDone")),
                Err(error) => ctx.report(&error),
            }
        });
    });
}

/// Asks first, since the app closes and opens again.
fn change_tor(ctx: &Ctx, change: impl FnOnce(&mut preferences::TorChoice)) {
    let mut choice = preferences::get().tor.unwrap_or_default();
    let before = choice;
    change(&mut choice);
    if choice == before {
        return;
    }
    let yes = actions::confirmed(
        &ctx.say("connection.restartTitle"),
        &ctx.say("connection.restartText"),
        &ctx.say("connection.restart"),
        &ctx.say("common.cancel"),
    );
    if !yes {
        page(ctx);
        return;
    }
    log::info!("Tor changed in settings: {}, by the {:?} route", if choice.on { "on" } else { "off" }, choice.route);
    preferences::change(|preferences| preferences.tor = Some(choice));
    guard::relaunch();
}
