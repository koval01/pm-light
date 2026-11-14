//! Sender lists, forwarding, domains, recovery and keys: what guards the mailbox.

use super::{Error, Method, Result, Service, Value, api, encoded, json, text};
use crate::{
    domain_check::{self, LiveCheck},
    model::{DomainStatus, Forwarding, KeyInfo, RecoveryStatus, SenderList, SenderRule},
};

const PAGE_SIZE: usize = 100;
/// Five thousand rows: more than a settings page should ever load at once.
const MAX_PAGES: usize = 50;

impl Service {
    /// Pages count from zero, and a short one is the last.
    async fn all_pages(&self, path: &str, key: &str) -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        for page in 0..MAX_PAGES {
            let reply: Value = self.client.get(&format!("{path}?Page={page}&PageSize={PAGE_SIZE}")).await?;
            let found = reply.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
            let last = found.len() < PAGE_SIZE;
            rows.extend(found);
            if last {
                break;
            }
        }
        Ok(rows)
    }

    pub async fn sender_rules(&self) -> Result<Vec<SenderRule>> {
        let rows = self.all_pages("/mail/v4/incomingdefaults", "IncomingDefaults").await?;
        Ok(rows.iter().filter_map(api::to_sender_rule).collect())
    }

    pub async fn add_sender_rule(&self, sender: &str, list: SenderList) -> Result<()> {
        let body = api::sender_rule_body(sender, list)?;
        self.client.act_with(Method::POST, "/mail/v4/incomingdefaults?Overwrite=1", body).await
    }

    pub async fn remove_sender_rule(&self, id: &str) -> Result<()> {
        self.client.act_with(Method::PUT, "/mail/v4/incomingdefaults/delete", json!({"IDs": [id]})).await
    }

    pub async fn forwardings(&self) -> Result<Vec<Forwarding>> {
        let (outgoing, incoming) = futures::try_join!(
            self.all_pages("/mail/v4/forwardings/outgoing", "OutgoingAddressForwardings"),
            self.all_pages("/mail/v4/forwardings/incoming", "IncomingAddressForwardings"),
        )?;
        let outgoing = outgoing.iter().map(|row| api::to_forwarding(row, true));
        let incoming = incoming.iter().map(|row| api::to_forwarding(row, false));
        Ok(outgoing.chain(incoming).collect())
    }

    pub async fn pause_forwarding(&self, id: &str, paused: bool) -> Result<()> {
        let action = if paused { "pause" } else { "resume" };
        self.client.act(Method::PUT, &format!("/mail/v4/forwardings/{}/{action}", encoded(id))).await
    }

    pub async fn delete_forwarding(&self, id: &str) -> Result<()> {
        self.client.act(Method::DELETE, &format!("/mail/v4/forwardings/{}", encoded(id))).await
    }

    /// A free plan is refused this list, which is the same as having no domains.
    pub async fn domains(&self) -> Result<Vec<DomainStatus>> {
        let rows = match self.all_pages("/core/v4/domains", "Domains").await {
            Err(Error::Api { status: 403, .. }) => Vec::new(),
            other => other?,
        };
        Ok(rows.iter().map(api::to_domain).collect())
    }

    /// What DNS says right now, without waiting for Proton's own periodic check.
    pub async fn check_domain(&self, name: &str) -> Result<LiveCheck> {
        let rows = self.all_pages("/core/v4/domains", "Domains").await?;
        let row =
            rows.iter().find(|row| text(row, "DomainName") == name).ok_or_else(|| Error::other("Unknown domain"))?;
        let owned = self.client.dns()?;
        let dns = &*owned;
        let dmarc_host = format!("_dmarc.{name}");
        let (root, mail, dmarc) = futures::try_join!(dns.txt(name), dns.mx(name), dns.txt(&dmarc_host))?;
        let keys = row.pointer("/DKIM/Config").and_then(Value::as_array).cloned().unwrap_or_default();
        let dkim = futures::future::try_join_all(keys.iter().map(|key| async move {
            let found = dns.cname(&format!("{}.{name}", text(key, "Hostname"))).await?;
            Ok::<_, Error>((text(key, "CNAME"), found))
        }))
        .await?;
        Ok(LiveCheck {
            name: name.to_string(),
            spf: domain_check::spf(&root),
            verified: domain_check::verified(root, &text(row, "VerifyCode")),
            mx: domain_check::mail_hosts(mail),
            dkim: domain_check::dkim(dkim),
            dmarc: domain_check::dmarc(&dmarc),
        })
    }

    pub async fn recovery(&self) -> Result<RecoveryStatus> {
        let (user, settings) = futures::try_join!(
            self.client.get::<Value>("/core/v4/users"),
            self.client.get::<Value>("/core/v4/settings")
        )?;
        let empty = Value::Null;
        Ok(api::to_recovery(user.get("User").unwrap_or(&empty), settings.get("UserSettings").unwrap_or(&empty)))
    }

    /// Fingerprints only. The keys themselves stay inside `crypto`.
    pub fn keys(&self) -> Vec<KeyInfo> {
        let addresses = self.addresses.read();
        let email_of = |id: &str| addresses.iter().find(|a| a.id == id).map(|a| a.email.clone());
        self.crypto
            .key_fingerprints()
            .into_iter()
            .map(|key| KeyInfo {
                address: key.address_id.as_deref().and_then(email_of).unwrap_or_default(),
                fingerprint: key.fingerprint,
                primary: key.primary,
            })
            .collect()
    }
}
