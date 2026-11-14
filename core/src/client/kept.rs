//! Answers to settings, kept for a while: they change rarely and are read on every visit to a tab. A kept answer is
//! shown at once, and past a minute it is asked for again behind the scenes.

use super::{API_ACCEPT, Client};
use crate::error::{Error, Result};
use serde_json::Value;
use std::time::{Duration, Instant};

const KEPT_ANSWER_FRESH: Duration = Duration::from_secs(60);
const KEPT_ANSWER_OLDEST: Duration = Duration::from_secs(3600);
const KEPT_PATHS: [&str; 9] = [
    "/core/v4/settings",
    "/mail/v4/settings",
    "/payments/",
    "/auth/v4/sessions",
    "/core/v4/domains",
    "/core/v4/forwarding",
    "/mail/v4/filters",
    "/mail/v4/incomingdefaults",
    "/mail/v4/forwardings",
];
/// Everyday work on mail changes no setting.
const NOT_SETTINGS: [&str; 3] = ["/mail/v4/messages", "/mail/v4/conversations", "/mail/v4/attachments"];

pub(super) struct KeptAnswer {
    body: Kept,
    at: Instant,
}

/// A 403 to a setting means the plan does not have it, which changes as rarely as the setting
/// itself, so it is kept the same way instead of being asked again on every page that shows it.
#[derive(Clone)]
pub(super) enum Kept {
    Answer(Value),
    NotInPlan { code: i64, message: String },
}

impl Kept {
    fn of(answer: &Result<Value>) -> Option<Self> {
        match answer {
            Ok(body) => Some(Self::Answer(body.clone())),
            Err(Error::Api { status: 403, code, message }) => {
                Some(Self::NotInPlan { code: *code, message: message.clone() })
            }
            Err(_) => None,
        }
    }

    pub(super) fn answer(self) -> Result<Value> {
        match self {
            Self::Answer(body) => Ok(body),
            Self::NotInPlan { code, message } => Err(Error::Api { status: 403, code, message }),
        }
    }
}

/// Whether answers to `path` are kept.
pub(super) fn is_kept(path: &str) -> bool {
    KEPT_PATHS.iter().any(|kept| path.starts_with(kept))
}

/// Whether `path`, changed, may change a setting.
pub(super) fn changes_settings(path: &str) -> bool {
    !NOT_SETTINGS.iter().any(|mail| path.starts_with(mail))
}

impl Client {
    pub(super) fn kept_answer(&self, path: &str) -> Option<Kept> {
        let mut kept = self.kept.lock();
        let answer = kept.get_mut(path)?;
        if answer.at.elapsed() > KEPT_ANSWER_OLDEST {
            kept.remove(path);
            return None;
        }
        let body = answer.body.clone();
        if answer.at.elapsed() > KEPT_ANSWER_FRESH {
            answer.at = Instant::now();
            drop(kept);
            self.renew_quietly(path);
        }
        Some(body)
    }

    pub(super) fn keep(&self, path: &str, answer: &Result<Value>) {
        if let Some(body) = Kept::of(answer) {
            self.kept.lock().insert(path.to_string(), KeptAnswer { body, at: Instant::now() });
        }
    }

    /// Nobody waits on this, so anything but a clean answer just leaves the kept one in place.
    fn renew_quietly(&self, path: &str) {
        let Ok(transport) = self.transport() else { return };
        let request = transport
            .http
            .client()
            .get(format!("{}{path}", transport.base))
            .headers(self.headers_for(false))
            .header("accept", API_ACCEPT);
        let (kept, path) = (self.kept.clone(), path.to_string());
        tokio::spawn(async move {
            let Ok(response) = request.send().await else { return };
            if !response.status().is_success() {
                return;
            }
            let Ok(body) = response.json::<Value>().await else { return };
            // Emptied meanwhile means a change or a sign-out: this answer may be from before it.
            let mut kept = kept.lock();
            if let Some(answer) = kept.get_mut(&path) {
                answer.body = Kept::Answer(body);
            }
        });
    }
}
