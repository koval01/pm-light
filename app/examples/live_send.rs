//! Sends test mail through the session the app saved, then waits for it to come back. Only the one
//! address the account's owner allowed may receive it, named in `LIVE_SEND_TO` so that it stays out of
//! the repository. Run it with `LIVE_SEND_TO=... cargo run -p proton-mail-light --example live_send`.

use keyring::Entry;
use proton_core::{
    AuthStep, PROTON_API, Service, SessionStore,
    model::{ListOptions, Outgoing, OutgoingFile},
};
use std::{sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};

const WAIT_ROUNDS: u32 = 30;
const WAIT_STEP: Duration = Duration::from_secs(6);

/// Reads the saved session and can change nothing, so the app stays signed in.
struct Borrowed(Option<String>);

impl SessionStore for Borrowed {
    fn load(&self) -> Option<String> {
        self.0.clone()
    }
    fn save(&self, _: &str) {}
    fn clear(&self) {}
}

fn outgoing(subject: &str, recipient: &str) -> Outgoing {
    Outgoing {
        draft_id: None,
        from: None,
        to: vec![recipient.to_string()],
        cc: vec![],
        bcc: vec![],
        subject: subject.to_string(),
        body: "<p>Proton Mail Light send check 😀</p>".to_string(),
        mime: "text/html".to_string(),
        files: vec![],
        existing: vec![],
        parent_id: None,
        action: None,
        deliver_at: None,
        auto_save_contacts: Some(false),
        delay_seconds: None,
        password: None,
        password_hint: None,
        expires_in: None,
        read_receipt: None,
    }
}

async fn send(service: &Service, mail: &Outgoing, allowed: &str) -> Result<String, String> {
    let recipients = mail.to.iter().chain(&mail.cc).chain(&mail.bcc);
    if recipients.into_iter().any(|r| r != allowed) {
        return Err("refusing: a recipient is not the allowed address".into());
    }
    service.send(mail).await.map_err(|e| e.to_string())
}

async fn run() -> Result<(), String> {
    let allowed = std::env::var("LIVE_SEND_TO").map_err(|_| "set LIVE_SEND_TO".to_string())?;
    let entry = Entry::new("org.koval-dev.proton-mail-light", "session").map_err(|e| e.to_string())?;
    let saved = entry.get_password().map_err(|e| format!("keychain: {e}"))?;
    let service =
        Service::new(PROTON_API, Arc::new(Borrowed(Some(saved))), Box::new(|_| {})).map_err(|e| e.to_string())?;
    service.forbid_token_refresh();
    if service.restore().await.map_err(|e| e.to_string())? != AuthStep::Ready {
        return Err("the saved session is not usable without a refresh".into());
    }

    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let data: Vec<u8> = (0..=255u8).cycle().take(20_000).collect();

    let subject = format!("Proton Mail Light test {stamp}");
    let mut mail = outgoing(&subject, &allowed);
    let staged_id = service.stage_file(data.clone());
    mail.files = vec![OutgoingFile {
        name: "café.bin".into(),
        mime: "application/octet-stream".into(),
        size: data.len() as u64,
        staged_id,
    }];
    println!("plain mail with a file: sent {}", !send(&service, &mail, &allowed).await?.is_empty());

    let protected_subject = format!("Proton Mail Light protected test {stamp}");
    let mut protected = outgoing(&protected_subject, &allowed);
    protected.password = Some(format!("proton-mail-light-{stamp}"));
    protected.password_hint = Some("test".into());
    protected.expires_in = Some(24 * 3600);
    println!("password, 24h expiry: sent {}", !send(&service, &protected, &allowed).await?.is_empty());

    for round in 1..=WAIT_ROUNDS {
        tokio_sleep(WAIT_STEP).await;
        // Forwarded mail may land in Spam, and the first copy to appear may be our own in Sent.
        let mut arrived = None;
        for folder in ["inbox", "spam"] {
            let options = ListOptions { limit: Some(15), ..Default::default() };
            let list = service.list(folder, &options).await.map_err(|e| e.to_string())?;
            arrived = arrived.or_else(|| list.into_iter().find(|m| m.subject.contains(&subject)));
        }
        let Some(meta) = arrived else { continue };
        println!("came back through Gmail after about {}s", u64::from(round) * WAIT_STEP.as_secs());
        let m = service.message(&meta.id).await.map_err(|e| e.to_string())?;
        println!("returned: text intact {}, signature {:?}, files {}", m.body.contains('😀'), m.signature, m.attachments.len());
        if let Some(file) = m.attachments.first() {
            let opened = service.attachment(&meta.id, file, &m.address_id).await.map_err(|e| e.to_string())?;
            println!("returned file identical: {}", opened == data);
        }
        return Ok(());
    }
    println!("did not come back within {}s", u64::from(WAIT_ROUNDS) * WAIT_STEP.as_secs());
    Ok(())
}

async fn tokio_sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
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
        Ok(()) => println!("send check finished"),
        Err(error) => println!("stopped: {error}"),
    }
}
