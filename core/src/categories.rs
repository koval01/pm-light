//! Proton's categories of the inbox: Primary, Social, Promotions, Newsletters, Transactions and Updates.
//!
//! Proton sorts the mail into them on its side; a client asks for the inbox one category at a time, shown as tabs, and
//! counts each. The ids, the order and the rules are those of Proton's web client (`MAILBOX_LABEL_IDS.CATEGORY_*`,
//! `categoryRank`, `useElements`, `selectCategoryUnreadCount`).

use crate::model::Counts;
use serde_json::Value;

/// What sorts nowhere else, and the mail of every category the person hid.
pub const PRIMARY: &str = "24";

/// Each category's label id and the word it goes by, in the order Proton shows them.
pub const ALL: [(&str, &str); 6] = [
    (PRIMARY, "primary"),
    ("20", "social"),
    ("21", "promotions"),
    ("25", "newsletters"),
    ("26", "transactions"),
    ("22", "updates"),
];

/// A category Proton dropped, which mail sorted before then may still carry, so Primary asks for it too:
/// `LEGACY_FORUM_CATEGORY_ID`.
const FORUM: &str = "23";

/// One category as the account has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub id: &'static str,
    pub name: &'static str,
    /// A tab of its own; a hidden category's mail shows in Primary. `Display`.
    pub shown: bool,
}

/// Whether the inbox is shown by category, and how.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Categories {
    /// The organization lets its people use them, as it always does without one: `MailCategoryViewEnabled`.
    pub allowed: bool,
    /// The person's own setting: `MailCategoryView`.
    pub chosen: bool,
    /// In Proton's order, only those the account has.
    pub list: Vec<Category>,
}

/// A category's word, for a label id that is one.
#[must_use]
pub fn name_of(id: &str) -> Option<&'static str> {
    ALL.iter().find(|(known, _)| *known == id).map(|(_, name)| *name)
}

/// A setting Proton sends as `true` or as `1`.
fn flag(value: Option<&Value>) -> bool {
    value.is_some_and(|value| value.as_bool().unwrap_or_else(|| value.as_i64().is_some_and(|number| number != 0)))
}

/// The categories among the account's system folders, `GET core/v4/labels?Type=4`, in Proton's order. Anything else
/// there is left out.
#[must_use]
pub fn from_labels(reply: &Value) -> Vec<Category> {
    let labels = reply.get("Labels").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    ALL.iter()
        .filter_map(|(id, name)| {
            let label = labels.iter().find(|label| label.get("ID").and_then(Value::as_str) == Some(id))?;
            Some(Category { id, name, shown: flag(label.get("Display")) })
        })
        .collect()
}

/// Whether an organization lets its people use categories, from `GET core/v4/organizations/settings`. Anything
/// unclear counts as no, as `getDefaultOrganizationSettings` has it.
#[must_use]
pub fn allowed_by(settings: &Value) -> bool {
    settings.get("MailCategoryViewEnabled").and_then(Value::as_bool).unwrap_or(false)
}

/// What shows or hides a category's tab, as Proton's settings send it (`updateLabel`): the label as Proton has it, with
/// `Display` changed and nothing else.
#[must_use]
pub fn shown_change(label: &Value, shown: bool) -> Value {
    let kept = |field: &str| label.get(field).cloned().unwrap_or(Value::Null);
    serde_json::json!({"Name": kept("Name"), "Color": kept("Color"), "Display": u8::from(shown), "Notify": kept("Notify")})
}

impl Categories {
    /// The person's setting, from the mail settings.
    pub fn read_settings(&mut self, mail_settings: &Value) {
        self.chosen = flag(mail_settings.get("MailCategoryView"));
    }

    /// Whether the inbox is shown by category. Without Primary among the labels there is nothing to show it by.
    #[must_use]
    pub fn on(&self) -> bool {
        self.allowed && self.chosen && self.list.iter().any(|category| category.id == PRIMARY)
    }

    /// The tabs, in order: Primary always, and every other category not hidden.
    #[must_use]
    pub fn tabs(&self) -> Vec<&Category> {
        self.list.iter().filter(|category| category.shown || category.id == PRIMARY).collect()
    }

    /// Whether a category has a tab now.
    #[must_use]
    pub fn has_tab(&self, id: &str) -> bool {
        self.on() && self.tabs().iter().any(|tab| tab.id == id)
    }

    fn hidden(&self) -> impl Iterator<Item = &Category> {
        self.list.iter().filter(|category| !category.shown && category.id != PRIMARY)
    }

    /// The labels a tab's mail wears besides the inbox's: Primary holds the hidden categories too. Nothing for what is
    /// no category, which leaves the whole inbox.
    #[must_use]
    pub fn labels_of(&self, tab: &str) -> Vec<String> {
        if tab != PRIMARY {
            return name_of(tab).map_or_default(|_| vec![tab.to_string()]);
        }
        let hidden = self.hidden().map(|category| category.id);
        std::iter::once(PRIMARY).chain(hidden).chain(std::iter::once(FORUM)).map(ToString::to_string).collect()
    }

    /// The mail not yet read in a tab, from counts asked with `OnlyInInboxForCategories`: Primary adds the hidden
    /// categories' to its own.
    #[must_use]
    pub fn unread(&self, tab: &str, counts: &Counts) -> u64 {
        let unread = |id: &str| counts.get(id).map_or(0, |count| count.unread);
        if tab == PRIMARY {
            self.hidden().fold(unread(PRIMARY), |sum, category| sum.saturating_add(unread(category.id)))
        } else {
            unread(tab)
        }
    }

    /// What the counts are asked with: whether the categories count only what is in the inbox. Not asked at all where
    /// the organization does not allow them, as `getCountQueryParams` has it.
    #[must_use]
    pub const fn count_parameter(&self) -> Option<u8> {
        if self.allowed { Some(if self.chosen { 1 } else { 0 }) } else { None }
    }
}

#[cfg(test)]
mod tests;
