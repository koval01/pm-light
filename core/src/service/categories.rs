//! The inbox by category: whether the account has its categories on, which it shows, and the counts of each tab.

use super::{Error, Method, Result, Service, Value, api, json};
use crate::categories::{self, Categories};

const ORGANIZATION_SETTINGS: &str = "/core/v4/organizations/settings";
/// `LABEL_TYPE.SYSTEM_FOLDER`, where the categories are.
const SYSTEM_FOLDERS: &str = "/core/v4/labels?Type=4";

impl Service {
    /// Whether the organization lets its people use categories. Without one it does, as Proton's client has a free
    /// account's stand-in organization say; with one it says so itself, and a failure is a no.
    async fn categories_allowed(&self) -> Result<bool> {
        if !api::may_have_organization(&self.user().await?) {
            return Ok(true);
        }
        Ok(self
            .client
            .get::<Value>(ORGANIZATION_SETTINGS)
            .await
            .is_ok_and(|settings| categories::allowed_by(&settings)))
    }

    /// When the mailbox opens, beside the mail settings, and again after a setting of theirs changed. Whatever fails
    /// leaves the inbox whole, as it was before categories.
    pub async fn read_categories(&self) {
        let (allowed, labels) = futures::join!(self.categories_allowed(), self.client.get::<Value>(SYSTEM_FOLDERS));
        let list = labels.map(|labels| categories::from_labels(&labels));
        if let Err(error) = &list {
            log::warn!("no categories: {}", error.kind());
        }
        let mut kept = self.categories.write();
        kept.allowed = allowed.unwrap_or(false);
        kept.list = list.unwrap_or_default();
        let (on, shown, all) = (kept.on(), kept.tabs().len(), kept.list.len());
        drop(kept);
        log::info!("categories: {on}, {shown} of {all} shown");
    }

    /// The person's own setting, as Proton's settings change it.
    pub async fn set_categories_on(&self, on: bool) -> Result<()> {
        let body = json!({"MailCategoryView": on});
        self.client.act_with(Method::PUT, "/mail/v4/settings/mail-category-view", body).await?;
        self.categories.write().chosen = on;
        Ok(())
    }

    /// A category's tab shown or hidden, as Proton's settings do it (`updateLabel`): the label goes back with its name,
    /// colour and notifications as Proton has them now, and `Display` changed. Primary keeps its tab.
    pub async fn show_category(&self, id: &str, shown: bool) -> Result<()> {
        if id == categories::PRIMARY || categories::name_of(id).is_none() {
            return Err(Error::other("Only a category besides Primary can be hidden"));
        }
        let reply: Value = self.client.get(SYSTEM_FOLDERS).await?;
        let labels = reply.get("Labels").and_then(Value::as_array);
        let label = labels
            .and_then(|labels| labels.iter().find(|label| label.get("ID").and_then(Value::as_str) == Some(id)))
            .ok_or_else(|| Error::other("The account has no such category"))?;
        let body = categories::shown_change(label, shown);
        self.client.act_with(Method::PUT, &format!("/core/v4/labels/{id}"), body).await?;
        if let Some(category) = self.categories.write().list.iter_mut().find(|category| category.id == id) {
            category.shown = shown;
        }
        Ok(())
    }

    /// The categories as last read.
    pub fn categories(&self) -> Categories {
        self.categories.read().clone()
    }
}

#[cfg(test)]
mod tests;
