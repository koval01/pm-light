//! Read-only look at the real account through the session the app saved. It sends only GET requests,
//! never refreshes the tokens, keeps nothing, and prints numbers and statuses: no subjects, addresses,
//! bodies or secrets. Run it with `cargo run -p proton-mail-light --example live_check`.

use keyring::Entry;
use proton_core::{AuthStep, PROTON_API, Service, SessionStore, model::ListOptions};
use std::{sync::Arc, time::Instant};

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
    println!("{name:<38} {value}");
}

async fn run() -> Result<(), String> {
    let entry = Entry::new("org.koval-dev.proton-mail-light", "session").map_err(|e| e.to_string())?;
    let saved = entry.get_password().map_err(|e| format!("keychain: {e}"))?;
    let service =
        Service::new(PROTON_API, Arc::new(Borrowed(Some(saved))), Box::new(|_| {})).map_err(|e| e.to_string())?;
    service.forbid_token_refresh();

    let started = Instant::now();
    let step = service.restore().await.map_err(|e| e.to_string())?;
    line("session reopened", format!("{:?} in {:?}", step, started.elapsed()));
    if step != AuthStep::Ready {
        return Err("the saved session is not usable without a refresh".into());
    }

    line("sending addresses", service.addresses().len());
    line("folders and labels", service.folders().await.map_err(|e| e.to_string())?.len());
    let counts = service.counts().await.map_err(|e| e.to_string())?;
    line("folders with counts", counts.len());
    let account = service.account().await.map_err(|e| e.to_string())?;
    line("paid plan read", account.plan.is_some());
    line("second factor", account.two_factor);
    let sessions = service.sessions().await.map_err(|e| e.to_string())?;
    line("sessions / this one found", format!("{} / {}", sessions.len(), sessions.iter().any(|s| s.current)));
    line("mail settings read", service.mail_settings().await.is_ok());
    line("filters read", service.filters().await.is_ok());
    line("contacts read", service.contact_emails().await.is_ok());

    let recent = service
        .list("inbox", &ListOptions { limit: Some(10), ..Default::default() })
        .await
        .map_err(|e| e.to_string())?;
    println!("\nlast {} messages (signature / html / files / images, trackers, links, removed / ms):", recent.len());
    for (index, meta) in recent.iter().enumerate() {
        let started = Instant::now();
        match service.message(&meta.id).await {
            Ok(m) => {
                let urls: Vec<String> = m.protection.images.iter().map(|i| i.url.clone()).collect();
                let images = urls.len();
                let trackers = service.image_trackers(urls).await.len();
                println!(
                    "  {index}: {:?} / {} / {} / {images}, {trackers}, {}, {} / {:?}",
                    m.signature,
                    m.html,
                    m.attachments.len(),
                    m.protection.links.len(),
                    m.protection.removed.len(),
                    started.elapsed(),
                );
            }
            Err(e) => println!("  {index}: FAILED {e}"),
        }
    }

    let with_files = ListOptions { limit: Some(1), has_attachments: Some(true), ..Default::default() };
    let found = service.list("inbox", &with_files).await.map_err(|e| e.to_string())?;
    if let Some(meta) = found.first() {
        let m = service.message(&meta.id).await.map_err(|e| e.to_string())?;
        if let Some(file) = m.attachments.first() {
            let started = Instant::now();
            match service.attachment(&meta.id, file, &m.address_id).await {
                Ok(data) => line(
                    "attachment decrypted, bytes / expected",
                    format!("{} / {} in {:?}", data.len(), file.size, started.elapsed()),
                ),
                Err(e) => line("attachment", format!("FAILED {e}")),
            }
        }
    }
    Ok(())
}

fn main() {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            println!("stopped: {error}");
            return;
        }
    };
    match runtime.block_on(run()) {
        Ok(()) => println!("\nread-only check finished"),
        Err(error) => println!("\nstopped: {error}"),
    }
}
