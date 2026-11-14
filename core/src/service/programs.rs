//! What an account has besides its mail: invitations, SMTP tokens, and the users of an organization.

use super::{Error, Method, Organization, Referrals, Result, Service, SmtpToken, Value, api, json};

const REFERRALS_SHOWN: usize = 100;
const MEMBERS_SHOWN: usize = 100;

/// A plan without this feature is answered with a refusal, which here only means there is nothing.
fn without_a_plan_for_it(answer: Result<Value>) -> Result<Value> {
    match answer {
        Err(Error::Api { status: 403 | 422, .. }) => Ok(Value::Null),
        other => other,
    }
}

impl Service {
    /// The list is the only part worth failing without: the rest is what the tab is for.
    pub async fn referrals(&self) -> Result<Referrals> {
        let (settings, status) = futures::try_join!(
            self.client.get::<Value>("/core/v4/settings"),
            self.client.get::<Value>("/core/v4/referrals/status")
        )?;
        let info = self.client.get::<Value>("/core/v4/referrals/info").await.unwrap_or(Value::Null);
        let path = format!("/core/v4/referrals?Offset=0&Limit={REFERRALS_SHOWN}");
        let list = self.client.get::<Value>(&path).await.unwrap_or(Value::Null);
        let link = settings.pointer("/UserSettings/Referral/Link").and_then(Value::as_str).unwrap_or_default();
        Ok(api::to_referrals(link, status.get("Status").unwrap_or(&status), &info, &list))
    }

    pub async fn smtp_tokens(&self) -> Result<Vec<SmtpToken>> {
        let reply: Value = self.client.get("/mail/v4/smtptokens").await?;
        Ok(api::to_smtp_tokens(&reply))
    }

    /// The code is shown once and kept nowhere, not even by Proton.
    pub async fn create_smtp_token(&self, address_id: &str, name: &str) -> Result<String> {
        let body = json!({"AddressID": address_id, "Name": name});
        let reply: Value = self.client.post("/mail/v4/smtptokens", body).await?;
        reply
            .get("SmtpTokenCode")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| Error::other("Proton sent no token"))
    }

    pub async fn delete_smtp_token(&self, id: &str) -> Result<()> {
        self.client.act(Method::DELETE, &format!("/mail/v4/smtptokens/{id}")).await
    }

    pub async fn organization(&self) -> Result<Organization> {
        let path = format!("/core/v4/members?Page=0&PageSize={MEMBERS_SHOWN}");
        let organization = without_a_plan_for_it(self.client.get::<Value>("/core/v4/organizations").await)?;
        // Asked only where there is an organization: without one Proton answers this with a 500.
        let members = if organization.is_null() {
            Value::Null
        } else {
            self.client.get::<Value>(&path).await.unwrap_or(Value::Null)
        };
        Ok(api::to_organization(organization.get("Organization").unwrap_or(&Value::Null), &members))
    }
}

#[cfg(test)]
mod tests;
