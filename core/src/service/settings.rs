//! Addresses, signatures, mail settings and filters.

use super::{
    AutoReply, Error, FILTER_ENABLED, FilterDraft, Habits, MailFilter, MailSettings, Method, OwnAddress, Result,
    Service, UserChoices, Value, api, escape, html, json, number, text, to_text,
};

impl Service {
    pub fn addresses(&self) -> Vec<OwnAddress> {
        self.addresses
            .read()
            .iter()
            .filter(|a| a.can_send)
            .map(|a| OwnAddress {
                id: a.id.clone(),
                email: a.email.clone(),
                display_name: a.display_name.clone(),
                signature: html::clean_signature(&a.signature),
                signature_text: to_text(&a.signature),
            })
            .collect()
    }

    pub async fn update_address(&self, id: &str, display_name: &str, signature_text: &str) -> Result<()> {
        let signature = escape(signature_text).replace('\n', "<br>");
        let body = json!({"DisplayName": display_name, "Signature": signature});
        self.client.act_with(Method::PUT, &format!("/core/v4/addresses/{id}"), body).await?;
        if let Some(address) = self.addresses.write().iter_mut().find(|a| a.id == id) {
            address.display_name = display_name.to_string();
            address.signature = signature;
        }
        Ok(())
    }

    pub async fn set_default_address(&self, id: &str) -> Result<()> {
        let ids: Vec<String> = self.addresses.read().iter().map(|a| a.id.clone()).collect();
        let order = api::with_default_first(&ids, id);
        self.client.act_with(Method::PUT, "/core/v4/addresses/order", json!({"AddressIDs": order})).await?;
        self.addresses.write().sort_by_key(|a| order.iter().position(|x| *x == a.id));
        Ok(())
    }

    pub(super) async fn raw_mail_settings(&self) -> Result<Value> {
        let reply: Value = self.client.get("/mail/v4/settings").await?;
        Ok(reply.get("MailSettings").cloned().unwrap_or(Value::Null))
    }

    pub async fn mail_settings(&self) -> Result<MailSettings> {
        let settings = api::to_mail_settings(&self.raw_mail_settings().await?);
        self.next_on_move.store(settings.next_message_on_move, std::sync::atomic::Ordering::Relaxed);
        Ok(settings)
    }

    pub async fn update_mail_setting(&self, key: &str, value: bool) -> Result<()> {
        let toggle = api::toggle(key).ok_or_else(|| Error::other(format!("Unknown setting {key}")))?;
        if key == "nextMessageOnMove" {
            self.next_on_move.store(value, std::sync::atomic::Ordering::Relaxed);
        }
        let body = json!({toggle.field: if value { toggle.on } else { toggle.off }});
        self.client.act_with(Method::PUT, &format!("/mail/v4/settings/{}", toggle.path), body).await
    }

    /// Where each setting of the table stands.
    pub async fn mail_choices(&self) -> Result<std::collections::HashMap<&'static str, usize>> {
        let raw = self.raw_mail_settings().await?;
        self.keep_habits(&raw);
        Ok(api::read_choices(&raw))
    }

    fn keep_habits(&self, raw: &Value) {
        let habits = api::to_habits(raw);
        self.view.lock().set_page(habits.page_size);
        *self.habits.write() = habits;
    }

    /// Once, when the mailbox opens. Each change here brings them up to date.
    pub async fn read_habits(&self) {
        if let Ok(raw) = self.raw_mail_settings().await {
            self.keep_habits(&raw);
        }
    }

    pub fn habits(&self) -> Habits {
        self.habits.read().clone()
    }

    pub async fn set_mail_choice(&self, key: &str, position: usize) -> Result<()> {
        let (path, body) =
            api::choice_change(key, position).ok_or_else(|| Error::other(format!("Unknown setting {key}")))?;
        self.client.act_with(Method::PUT, &path, body).await?;
        self.read_habits().await;
        Ok(())
    }

    pub async fn user_choices(&self) -> Result<UserChoices> {
        let reply: Value = self.client.get("/core/v4/settings").await?;
        Ok(api::to_user_choices(reply.get("UserSettings").unwrap_or(&Value::Null)))
    }

    pub async fn set_user_flag(&self, key: &str, on: bool) -> Result<()> {
        let (_, path, field) = api::USER_FLAGS
            .iter()
            .find(|(name, ..)| *name == key)
            .ok_or_else(|| Error::other(format!("Unknown setting {key}")))?;
        self.client.act_with(Method::PUT, &format!("/core/v4/settings/{path}"), json!({*field: u8::from(on)})).await
    }

    pub async fn set_news(&self, name: &str, on: bool) -> Result<()> {
        if !api::NEWS.iter().any(|(known, _)| *known == name) {
            return Err(Error::other(format!("Unknown subscription {name}")));
        }
        self.client.act_with(Method::PATCH, "/core/v4/settings/news", json!({name: on})).await
    }

    pub async fn set_breach_alerts(&self, on: bool) -> Result<()> {
        self.client.act(if on { Method::POST } else { Method::DELETE }, "/core/v4/settings/breachalerts").await
    }

    pub async fn set_invoice_text(&self, text: &str) -> Result<()> {
        self.client.act_with(Method::PUT, "/core/v4/settings/invoicetext", json!({"InvoiceText": text})).await
    }

    pub async fn update_delay_send(&self, seconds: u64) -> Result<()> {
        let body = json!({"DelaySendSeconds": seconds});
        self.client.act_with(Method::PUT, "/mail/v4/settings/delaysend", body).await
    }

    pub async fn auto_reply(&self) -> Result<AutoReply> {
        let settings = self.raw_mail_settings().await?;
        Ok(api::to_auto_reply(settings.get("AutoResponder")))
    }

    pub async fn update_auto_reply(&self, reply: &AutoReply, zone: &str) -> Result<()> {
        let body = json!({"AutoResponder": api::from_auto_reply(reply, zone)});
        self.client.act_with(Method::PUT, "/mail/v4/settings/autoresponder", body).await
    }

    pub async fn filters(&self) -> Result<Vec<MailFilter>> {
        let reply: Value = self.client.get("/mail/v4/filters").await?;
        let rows = reply.get("Filters").and_then(Value::as_array);
        Ok(rows.map_or_else(Vec::new, |rows| {
            rows.iter()
                .map(|f| MailFilter {
                    id: text(f, "ID"),
                    name: text(f, "Name"),
                    enabled: number(f, "Status") == FILTER_ENABLED,
                    sieve: text(f, "Sieve"),
                })
                .collect()
        }))
    }

    /// Asks Proton to validate the script first; returns its complaints, empty once saved.
    pub async fn save_filter(&self, filter: &FilterDraft) -> Result<Vec<String>> {
        let check = json!({"Version": api::SIEVE_VERSION, "Sieve": filter.sieve});
        let reply: Value = self.client.put("/mail/v4/filters/check", check).await?;
        if let Some(issues) = reply.get("Issues").and_then(Value::as_array).filter(|i| !i.is_empty()) {
            return Ok(issues.iter().map(api::issue_text).collect());
        }
        let body = json!({
            "Name": filter.name,
            "Status": u8::from(filter.enabled.unwrap_or(true)),
            "Version": api::SIEVE_VERSION,
            "Sieve": filter.sieve,
        });
        match &filter.id {
            Some(id) => self.client.act_with(Method::PUT, &format!("/mail/v4/filters/{id}"), body).await?,
            None => self.client.act_with(Method::POST, "/mail/v4/filters", body).await?,
        }
        Ok(Vec::new())
    }

    pub async fn set_filter_enabled(&self, id: &str, on: bool) -> Result<()> {
        let action = if on { "enable" } else { "disable" };
        self.client.act(Method::PUT, &format!("/mail/v4/filters/{id}/{action}")).await
    }

    pub async fn delete_filter(&self, id: &str) -> Result<()> {
        self.client.act(Method::DELETE, &format!("/mail/v4/filters/{id}")).await
    }

    pub async fn mark_not_spam(&self, id: &str) -> Result<()> {
        self.client.act(Method::PUT, &format!("/mail/v4/messages/{id}/mark/ham")).await
    }

    pub async fn block_sender(&self, email: &str) -> Result<()> {
        let body = json!({"Email": email, "Location": api::LOCATION_BLOCKED});
        self.client.act_with(Method::POST, "/mail/v4/incomingdefaults?Overwrite=1", body).await
    }

    pub async fn unsubscribe(&self, id: &str) -> Result<()> {
        self.client.act(Method::POST, &format!("/mail/v4/messages/{id}/unsubscribe")).await
    }

    pub async fn send_read_receipt(&self, id: &str) -> Result<()> {
        self.client.act(Method::POST, &format!("/mail/v4/messages/{id}/receipt")).await
    }

    pub async fn snooze(&self, conversation_id: &str, until: u64) -> Result<()> {
        let ids = [conversation_id.to_string()];
        let body = json!({"SnoozeTime": until});
        self.batch("/mail/v4/conversations/snooze", &ids, "Could not snooze", body).await
    }

    pub async fn unsnooze(&self, conversation_id: &str) -> Result<()> {
        let ids = [conversation_id.to_string()];
        self.batch("/mail/v4/conversations/unsnooze", &ids, "Could not unsnooze", json!({})).await
    }
}
