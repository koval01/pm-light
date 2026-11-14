//! Read-only look at the real account through the session the app saved. It sends only GET requests,
//! never refreshes the tokens, keeps nothing, and prints numbers and statuses: no subjects, addresses,
//! bodies or secrets. Run it with `cargo run -p proton-mail-light --example live_check`.
//!
//! The session comes from the keychain, or from `LIVE_SESSION` when that is set, as in CI. With `LIVE_SESSION` its
//! output is public: errors are then named by their kind, never by their text.

use keyring::Entry;
use proton_core::{AuthStep, PROTON_API, Service, SessionStore, model::ListOptions};
use std::io::Write as _;
use std::{sync::Arc, time::Instant};

/// A line of the report, to the terminal that runs this; a closed one is no reason to stop.
fn say(text: &str) {
    let _ = writeln!(std::io::stdout().lock(), "{text}");
}

/// Reads the saved session and can change nothing: not even a failure may clear the app's sign-in.
struct Borrowed(Option<String>);

impl SessionStore for Borrowed {
    fn load(&self) -> Option<String> {
        self.0.clone()
    }
    fn save(&self, _: &str) {}
    fn clear(&self) {}
}

fn line(name: &str, value: impl std::fmt::Display) {
    say(&format!("{name:<38} {value}"));
}

/// The session to borrow: from the environment in CI, else from the keychain.
fn saved_session() -> Result<String, String> {
    if let Some(session) = std::env::var("LIVE_SESSION").ok().filter(|value| !value.trim().is_empty()) {
        return Ok(session);
    }
    let entry = Entry::new("org.koval-dev.proton-mail-light", "session").map_err(|e| e.to_string())?;
    entry.get_password().map_err(|e| format!("keychain: {e}"))
}

/// An error as the output may show it: its kind when the output is public, its text on the owner's own machine.
fn shown(error: &proton_core::Error) -> String {
    if std::env::var_os("LIVE_SESSION").is_some() { error.kind().to_string() } else { error.to_string() }
}

async fn run() -> Result<(), String> {
    let saved = saved_session()?;
    let service = Service::new(PROTON_API, Arc::new(Borrowed(Some(saved))), Box::new(|_| {})).map_err(|e| shown(&e))?;
    service.forbid_token_refresh();

    let started = Instant::now();
    let step = service.restore().await.map_err(|e| shown(&e))?;
    line("session reopened", format!("{:?} in {:?}", step, started.elapsed()));
    if step != AuthStep::Ready {
        return Err("the saved session is not usable without a refresh".into());
    }

    line("sending addresses", service.addresses().len());
    line("folders and labels", service.folders().await.map_err(|e| shown(&e))?.len());
    let counts = service.counts().await.map_err(|e| shown(&e))?;
    line("folders with counts", counts.len());
    let account = service.account().await.map_err(|e| shown(&e))?;
    line("paid plan read", account.plan.is_some());
    line("second factor", account.two_factor);
    let sessions = service.sessions().await.map_err(|e| shown(&e))?;
    line("sessions / this one found", format!("{} / {}", sessions.len(), sessions.iter().any(|s| s.current)));
    line("mail settings read", service.mail_settings().await.is_ok());
    line("filters read", service.filters().await.is_ok());
    line("contacts read", service.contact_emails().await.is_ok());

    let recent =
        service.list("inbox", &ListOptions { limit: Some(10), ..Default::default() }).await.map_err(|e| shown(&e))?;
    say(&format!(
        "\nlast {} messages (signature / html / files / images, trackers, links, removed / ms):",
        recent.len()
    ));
    for (index, meta) in recent.iter().enumerate() {
        let started = Instant::now();
        match service.message(&meta.id).await {
            Ok(m) => {
                let urls: Vec<String> = m.protection.images.iter().map(|i| i.url.clone()).collect();
                let images = urls.len();
                let trackers = service.image_trackers(urls).await.len();
                say(&format!(
                    "  {index}: {:?} / {} / {} / {images}, {trackers}, {}, {} / {:?}",
                    m.signature,
                    m.html,
                    m.attachments.len(),
                    m.protection.links.len(),
                    m.protection.removed.len(),
                    started.elapsed(),
                ));
            }
            Err(e) => say(&format!("  {index}: FAILED {}", shown(&e))),
        }
    }

    let with_files = ListOptions { limit: Some(1), has_attachments: Some(true), ..Default::default() };
    let found = service.list("inbox", &with_files).await.map_err(|e| shown(&e))?;
    if let Some(meta) = found.first() {
        let m = service.message(&meta.id).await.map_err(|e| shown(&e))?;
        if let Some(file) = m.attachments.first() {
            let started = Instant::now();
            match service.attachment(&meta.id, file, &m.address_id).await {
                Ok(data) => line(
                    "attachment decrypted, bytes / expected",
                    format!("{} / {} in {:?}", data.len(), file.size, started.elapsed()),
                ),
                Err(e) => line("attachment", format!("FAILED {}", shown(&e))),
            }
        }
    }
    Ok(())
}

fn main() {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            say(&format!("stopped: {error}"));
            return;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => say("\nread-only check finished"),
        Err(error) => say(&format!("\nstopped: {error}")),
    }
}
