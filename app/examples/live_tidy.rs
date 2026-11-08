//! Readies the throwaway test account for checks whose pictures anyone may see: mail in its inbox that Proton itself
//! did not send is moved to the trash and deleted for good, and a filter sends every later mail from anyone but
//! Proton to spam, so that a stranger who reads the address off a picture cannot put words into the next one.
//!
//! Only for that account. It sends nothing and never refreshes the tokens. Run it with
//! `cargo run -p proton-mail-light --example live_tidy`, which only counts and sets the filter; `-- --delete` also
//! deletes, and `-- --sample-draft` saves a draft with no recipient for the checks to draw when the account holds no
//! other mail. macOS asks before it lets the session out of the keychain.

use keyring::Entry;
use proton_core::{
    AuthStep, PROTON_API, Service, SessionStore,
    compose::{ComposeMode, ComposeState},
    model::{FilterDraft, ListOptions},
};
use std::sync::Arc;

/// Where Proton's own mail comes from.
const PROTON_DOMAINS: [&str; 4] = ["proton.me", "protonmail.com", "protonmail.ch", "pm.me"];
const FILTER_NAME: &str = "Only Proton's own mail stays in the inbox";
/// A draft with no recipient, for a check to open and draw when the account holds no other mail: it can go nowhere.
const SAMPLE_SUBJECT: &str = "A sample mail";
const SAMPLE_BODY: &str = "<h2 style=\"color:#2f62dc\">A sample mail</h2><p>This draft exists only for the checks of \
Proton Mail Light to open and draw. Its text has <b>bold</b>, <i>italic</i> and <a href=\"https://example.com\">a link</a>.\
</p><table border=\"1\" cellpadding=\"6\" style=\"border-collapse:collapse\"><tr><td>Item 1</td><td>12.00</td></tr>\
<tr><td>Item 2</td><td>24.00</td></tr></table><ol><li>A line.</li><li>Another line.</li><li>A third line.</li></ol>\
<p style=\"background:#fff3c4;padding:8px\">The end of the sample.</p>";

struct Borrowed(Option<String>);

impl SessionStore for Borrowed {
    fn load(&self) -> Option<String> {
        self.0.clone()
    }
    fn save(&self, _: &str) {}
    fn clear(&self) {}
}

/// Proton writes from its domains and their subdomains, such as `welcome.proton.me` and `notify.proton.me`.
fn from_proton(email: &str) -> bool {
    let domain = email.rsplit_once('@').map_or("", |(_, domain)| domain).to_ascii_lowercase();
    PROTON_DOMAINS.iter().any(|own| domain == *own || domain.ends_with(&format!(".{own}")))
}

fn sieve() -> String {
    let exact = PROTON_DOMAINS.map(|domain| format!("\"{domain}\"")).join(", ");
    let under = PROTON_DOMAINS.map(|domain| format!("\"*.{domain}\"")).join(", ");
    format!(
        "require [\"fileinto\"];\nif not anyof (address :domain :is \"from\" [{exact}], address :domain :matches \"from\" [{under}]) {{\n    fileinto \"spam\";\n    stop;\n}}\n"
    )
}

async fn run() -> Result<(), String> {
    let saved = Entry::new("org.koval-dev.proton-mail-light", "session")
        .and_then(|entry| entry.get_password())
        .map_err(|e| format!("keychain: {e}"))?;
    let service = Service::new(PROTON_API, Arc::new(Borrowed(Some(saved))), Box::new(|_| {})).map_err(|e| e.to_string())?;
    service.forbid_token_refresh();
    if service.restore().await.map_err(|e| e.to_string())? != AuthStep::Ready {
        return Err("the saved session is not usable without a refresh".into());
    }

    service.view_open("inbox").await;
    let rows = service.list("inbox", &ListOptions { limit: Some(150), ..Default::default() }).await.map_err(|e| e.to_string())?;
    let strangers: Vec<String> = rows.iter().filter(|row| !from_proton(&row.from.email)).map(|row| row.id.clone()).collect();
    println!("inbox: {} mails, {} not from Proton", rows.len(), strangers.len());
    if !strangers.is_empty() && std::env::args().any(|argument| argument == "--delete") {
        service.view_move(Some(strangers.clone()), "trash").await.map_err(|e| e.to_string())?;
        service.view_open("trash").await;
        service.view_delete(Some(strangers.clone())).await.map_err(|e| e.to_string())?;
        println!("deleted for good: {}", strangers.len());
    }

    if std::env::args().any(|argument| argument == "--sample-draft") {
        let drafts = service.list("drafts", &ListOptions { limit: Some(50), ..Default::default() }).await.map_err(|e| e.to_string())?;
        if drafts.iter().any(|draft| draft.subject == SAMPLE_SUBJECT) {
            println!("the sample draft is there already");
        } else {
            let draft = ComposeState {
                mode: ComposeMode::New,
                draft_id: None,
                from: None,
                to: String::new(),
                cc: String::new(),
                bcc: String::new(),
                subject: SAMPLE_SUBJECT.to_string(),
                body: SAMPLE_BODY.to_string(),
                parent_id: None,
                action: None,
                existing: Vec::new(),
                files: Vec::new(),
                plain: false,
            };
            service.save_draft(&draft).await.map_err(|e| e.to_string())?;
            println!("a sample draft saved, with no recipient");
        }
    }

    let filters = service.filters().await.map_err(|e| e.to_string())?;
    let kept = filters.iter().find(|filter| filter.name == FILTER_NAME).map(|filter| filter.id.clone());
    let draft = FilterDraft { id: kept, enabled: Some(true), name: FILTER_NAME.to_string(), sieve: sieve() };
    let issues = service.save_filter(&draft).await.map_err(|e| e.to_string())?;
    if issues.is_empty() {
        println!("filter saved: mail from anyone but Proton goes to spam");
    } else {
        println!("Proton refused the filter: {}", issues.join("; "));
    }
    Ok(())
}

fn main() {
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread().enable_all().build() else {
        println!("stopped: no runtime");
        return;
    };
    if let Err(error) = runtime.block_on(run()) {
        println!("stopped: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proton_is_known_by_the_domain_alone() {
        assert!(from_proton("no-reply@proton.me") && from_proton("x@PM.ME"));
        assert!(from_proton("welcome@welcome.proton.me") && from_proton("a@notify.protonmail.ch"), "subdomains are Proton's too");
        assert!(!from_proton("someone@koval-dev.org") && !from_proton("x@proton.me.evil.example") && !from_proton("noat"));
    }

    #[test]
    fn the_filter_sends_everyone_else_to_spam() {
        let sieve = sieve();
        assert!(sieve.starts_with("require [\"fileinto\"];"));
        assert!(sieve.contains("address :domain :is \"from\" [\"proton.me\", \"protonmail.com\""));
        assert!(sieve.contains("address :domain :matches \"from\" [\"*.proton.me\""));
        assert!(sieve.contains("fileinto \"spam\";"));
    }
}
