//! The account, its subscription, sessions and sign-in log.

use super::{Account, AccountSession, api, AUTH_LOG_ENTRIES, AuthLogEntry, Billing, Error, json, Method, number, Result, Service, Storage, TimeSetting, TimeSettings, Value};

impl Service {
    pub async fn storage(&self) -> Result<Storage> {
        let reply: Value = self.client.get("/core/v4/users").await?;
        let user = reply.get("User").unwrap_or(&Value::Null);
        Ok(Storage { used: number(user, "UsedSpace"), max: number(user, "MaxSpace") })
    }

    pub async fn time_settings(&self) -> Result<TimeSettings> {
        let reply: Value = self.client.get("/core/v4/settings").await?;
        let settings = reply.get("UserSettings").unwrap_or(&Value::Null);
        let read = |name| u8::try_from(number(settings, name)).unwrap_or(0);
        let found = TimeSettings {
            time_format: read("TimeFormat"),
            date_format: read("DateFormat"),
            week_start: read("WeekStart"),
        };
        crate::timing::set_week_start(found.week_start);
        crate::timing::set_clock_format(found.time_format);
        crate::timing::set_date_format(found.date_format);
        Ok(found)
    }

    pub async fn update_time_setting(&self, setting: TimeSetting, value: u8) -> Result<()> {
        let (path, name, highest) = match setting {
            TimeSetting::TimeFormat => ("/core/v4/settings/timeformat", "TimeFormat", 2),
            TimeSetting::DateFormat => ("/core/v4/settings/dateformat", "DateFormat", 3),
            TimeSetting::WeekStart => ("/core/v4/settings/weekstart", "WeekStart", 7),
        };
        if value > highest {
            return Err(Error::other("No such setting"));
        }
        self.client.act_with(Method::PUT, path, json!({ name: value })).await?;
        match setting {
            TimeSetting::TimeFormat => crate::timing::set_clock_format(value),
            TimeSetting::DateFormat => crate::timing::set_date_format(value),
            TimeSetting::WeekStart => crate::timing::set_week_start(value),
        }
        Ok(())
    }

    pub async fn account(&self) -> Result<Account> {
        let (user, settings) = futures::try_join!(
            self.client.get::<Value>("/core/v4/users"),
            self.client.get::<Value>("/core/v4/settings")
        )?;
        let user = user.get("User").cloned().unwrap_or(Value::Null);
        let settings = settings.get("UserSettings").cloned().unwrap_or(Value::Null);
        // A free account has no subscription to ask about, and the rest stays useful if this fails.
        let subscription = if number(&user, "Subscribed") != 0 {
            self.client
                .get::<Value>("/payments/v4/subscription")
                .await
                .ok()
                .and_then(|reply| reply.get("Subscription").cloned())
        } else {
            None
        };
        Ok(api::to_account(&user, &settings, subscription.as_ref()))
    }

    /// Each part stands alone: an account with no payment history still has a plan to show.
    pub async fn billing(&self) -> Result<Billing> {
        let reply: Value = self.client.get("/core/v4/users").await?;
        let user = reply.get("User").cloned().unwrap_or(Value::Null);
        let subscribed = number(&user, "Subscribed") != 0;
        let list = |path: &'static str, key: &'static str| async move {
            let reply = self.client.get::<Value>(path).await.ok();
            reply.and_then(|r| r.get(key).and_then(Value::as_array).cloned()).unwrap_or_default()
        };
        let subscription = async {
            if !subscribed {
                return None;
            }
            let reply = self.client.get::<Value>("/payments/v4/subscription").await.ok()?;
            reply.get("Subscription").cloned()
        };
        let (subscription, methods, invoices) = futures::join!(
            subscription,
            list("/payments/v5/methods", "PaymentMethods"),
            list("/payments/v5/invoices?Page=0&PageSize=10&Owner=0&Document=invoice", "Invoices"),
        );
        Ok(api::to_billing(&user, subscription.as_ref(), &methods, &invoices))
    }

    pub async fn sessions(&self) -> Result<Vec<AccountSession>> {
        let reply: Value = self.client.get("/auth/v4/sessions").await?;
        let rows = reply.get("Sessions").and_then(Value::as_array).cloned().unwrap_or_default();
        let current = self.client.session().map(|s| s.uid.clone());
        Ok(api::to_sessions(&rows, current.as_deref()))
    }

    pub async fn revoke_session(&self, id: &str) -> Result<()> {
        self.client.act(Method::DELETE, &format!("/auth/v4/sessions/{id}")).await
    }

    pub async fn revoke_other_sessions(&self) -> Result<()> {
        self.client.act(Method::DELETE, "/auth/v4/sessions").await
    }

    pub async fn auth_log(&self) -> Result<Vec<AuthLogEntry>> {
        let path = format!("/core/v4/logs/auth?Page=0&PageSize={AUTH_LOG_ENTRIES}");
        let reply: Value = self.client.get(&path).await?;
        let rows = reply.get("Logs").and_then(Value::as_array);
        Ok(rows.map_or_else(Vec::new, |rows| rows.iter().map(api::to_auth_log).collect()))
    }

    pub async fn update_auth_log_mode(&self, mode: u8) -> Result<()> {
        self.client.act_with(Method::PUT, "/core/v4/settings/logauth", json!({"LogAuth": mode})).await
    }

    pub async fn clear_auth_log(&self) -> Result<()> {
        self.client.act(Method::DELETE, "/core/v4/logs/auth").await
    }
}
