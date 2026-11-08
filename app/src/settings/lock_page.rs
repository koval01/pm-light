//! The app password and what goes with it. Whoever has this screen
//! can already read the mail, so only removing the password asks for it.

use super::{
    Ctx,
    rows::{ghost, note, secret, section, segmented, switch},
};
use crate::{SettingsSection, preferences, session};
use proton_core::error::Error;
use zeroize::Zeroizing;

const AUTO_LOCK: [u64; 5] = [0, 1, 5, 15, 60];
const MIN_PASSWORD: usize = 4;
const PASSWORD: &str = "lock-password";
const REPEAT: &str = "lock-repeat";
const SAVE: &str = "lock-save";

fn ready(ctx: &Ctx) -> bool {
    let password = ctx.text(PASSWORD);
    if ctx.service.app_lock_enabled() {
        return !password.is_empty();
    }
    password.chars().count() >= MIN_PASSWORD && password == ctx.text(REPEAT)
}

pub fn section_of(ctx: &Ctx) -> SettingsSection {
    let enabled = ctx.service.app_lock_enabled();
    let mut lines = vec![note(&ctx.say("lock.about"))];
    if enabled {
        lines.push(secret(PASSWORD, &ctx.say("lock.current")));
    } else {
        lines.push(secret(PASSWORD, &ctx.say("lock.new")));
        lines.push(secret(REPEAT, &ctx.say("lock.repeat")));
        lines.push(note(&ctx.say("lock.warning")));
    }
    lines.push(ghost(SAVE, &ctx.say(if enabled { "lock.remove" } else { "lock.set" }), false));
    if enabled {
        let options: Vec<String> = AUTO_LOCK
            .iter()
            .map(|minutes| match minutes {
                0 => ctx.say("settings.undoOff"),
                count => ctx.say_with("lock.minutes", &[("count", &count.to_string())]),
            })
            .collect();
        let minutes = preferences::get().auto_lock_minutes;
        let selected = AUTO_LOCK.iter().position(|known| *known == minutes).unwrap_or(0);
        lines.push(segmented("lock-auto", &ctx.say("lock.auto"), options, selected));
        if session::touch_id::available() {
            lines.push(switch("lock-touch", &ctx.say("lock.touchIdOption"), &ctx.say("lock.touchIdHint"), session::has_unlock_key()));
        }
    }
    section("lock", &ctx.say("lock.section"), "", lines)
}

fn save(ctx: &Ctx) {
    let enabled = ctx.service.app_lock_enabled();
    let (service, password) = (ctx.service.clone(), Zeroizing::new(ctx.text(PASSWORD)));
    let answer = ctx.clone();
    tokio::spawn(async move {
        let done = if enabled {
            service.remove_app_password(&password).await
        } else {
            session::forget_unlock_key();
            service.set_app_password(&password).await
        };
        if done.is_ok() && enabled {
            session::forget_unlock_key();
        }
        let app = answer.app.clone();
        let _ = app.upgrade_in_event_loop(move |_| match done {
            Ok(()) => {
                answer.tell(&answer.say(if enabled { "lock.removed" } else { "lock.saved" }));
                super::redraw(&answer);
            }
            Err(Error::WrongPassword) => answer.tell(&answer.say("lock.wrong")),
            Err(error) => answer.report(&error),
        });
    });
}

fn set_touch_id(ctx: &Ctx, on: bool) {
    if !on {
        session::forget_unlock_key();
        super::redraw(ctx);
        return;
    }
    let Some(key) = ctx.service.lock_key() else { return };
    let reason = ctx.say("lock.touchIdReason");
    let answer = ctx.clone();
    tokio::spawn(async move {
        let confirmed = tokio::task::spawn_blocking(move || session::touch_id::confirm(&reason)).await.unwrap_or(false);
        let stored = confirmed && session::store_unlock_key(key.as_slice());
        let app = answer.app.clone();
        let _ = app.upgrade_in_event_loop(move |_| {
            if confirmed && !stored {
                answer.tell(&answer.say("errors.internal"));
            }
            super::redraw(&answer);
        });
    });
}

/// Whether this was about the lock.
pub fn heard(ctx: &Ctx, id: &str, value: i32) -> bool {
    match id {
        SAVE => save(ctx),
        "lock-auto" => {
            let minutes = usize::try_from(value).ok().and_then(|position| AUTO_LOCK.get(position)).copied().unwrap_or(0);
            preferences::change(|preferences| preferences.auto_lock_minutes = minutes);
        }
        "lock-touch" => set_touch_id(ctx, value == 1),
        _ => return false,
    }
    true
}

pub fn typed(ctx: &Ctx) {
    ctx.enable(SAVE, ready(ctx));
}
