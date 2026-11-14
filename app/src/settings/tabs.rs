//! The order and names of Proton's own settings, so that nothing has to be learned twice.

use crate::TabRow;
use proton_core::ranking;

pub struct Tab {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    /// What else a search may find this tab by.
    pub about: &'static [&'static str],
    /// Lives on Proton's site: the page there, under `/u/*/mail/`.
    pub web: &'static str,
}

const fn tab(id: &'static str, title: &'static str, icon: &'static str, about: &'static [&'static str]) -> Tab {
    Tab { id, title, icon, about, web: "" }
}

const fn web(id: &'static str, title: &'static str, icon: &'static str, web: &'static str) -> Tab {
    Tab { id, title, icon, about: &[], web }
}

pub const GROUPS: [(&str, &[Tab]); 4] = [
    (
        "tabs.groupAccount",
        &[
            tab("overview", "tabs.overview", "house", &["settings.signOut", "account.plan"]),
            tab("subscription", "tabs.subscription", "credit-card", &["billing.invoices", "billing.methods"]),
            tab("recovery", "tabs.recovery", "key-round", &["recovery.phrase", "recovery.phone"]),
            tab("account", "tabs.account", "user-round", &["security.2fa", "security.recovery"]),
            tab("language", "tabs.language", "languages", &["settings.language"]),
            tab("appearance", "tabs.appearance", "palette", &["settings.theme"]),
            tab("security", "tabs.security", "shield", &["lock.section", "security.log"]),
            tab("referral", "tabs.referral", "gift", &["referral.link"]),
            web("import", "tabs.import", "file-input", "easy-switch"),
        ],
    ),
    (
        "tabs.groupMail",
        &[
            tab("apps", "tabs.apps", "download", &["apps.bridge"]),
            tab(
                "messages",
                "tabs.messages",
                "mail",
                &["settings.reading", "settings.writing", "settings.undoSend", "settings.hideRemote"],
            ),
            tab("privacy", "tabs.privacy", "shield-check", &["settings.hideRemote", "settings.confirmLinks"]),
            tab("identity", "tabs.identity", "id-card", &["settings.addresses"]),
            tab("folders", "tabs.folders", "folder", &["nav.folders", "nav.labels"]),
            tab("filters", "tabs.filters", "funnel", &["senders.title"]),
            tab("forwarding", "tabs.forwarding", "forward", &["forwarding.title"]),
            tab("domains", "tabs.domains", "globe", &[]),
            tab("encryption", "tabs.encryption", "lock-keyhole", &["keys.about"]),
            tab("imap", "tabs.imap", "server", &["smtp.title"]),
            tab("backup", "tabs.backup", "hard-drive-download", &[]),
        ],
    ),
    ("tabs.groupOrganization", &[tab("multiUser", "tabs.multiUser", "users", &["org.users"])]),
    (
        "tabs.groupApp",
        &[
            tab(
                "application",
                "tabs.application",
                "monitor",
                &["settings.notifications", "settings.autostart", "privacy.hide", "privacy.gravatar", "tour.again"],
            ),
            tab("connection", "tabs.connection", "activity", &["connection.proton", "connection.quad9"]),
            tab("tor", "tabs.tor", "circuit", &["connection.tor", "connection.torRoute", "connection.torNew"]),
            tab("journal", "tabs.journal", "scroll-text", &[]),
            tab("printCheck", "tabs.printCheck", "qr-code", &["printCheck.scan", "print.seal"]),
            tab("keyboard", "tabs.keyboard", "keyboard", &["settings.shortcuts"]),
            tab("about", "tabs.about", "info", &[]),
        ],
    ),
];

pub fn all() -> impl Iterator<Item = &'static Tab> {
    GROUPS.iter().flat_map(|(_, tabs)| tabs.iter())
}

pub fn find(id: &str) -> Option<&'static Tab> {
    all().find(|tab| tab.id == id)
}

pub fn web_address(slug: &str) -> String {
    format!("https://account.proton.me/u/*/mail/{slug}")
}

/// The tabs a search left, group by group, without the groups it emptied.
pub fn shown(say: &dyn Fn(&str) -> String, query: &str) -> Vec<TabRow> {
    let tabs: Vec<&Tab> = all().collect();
    let texts: Vec<String> = tabs
        .iter()
        .map(|tab| std::iter::once(tab.title).chain(tab.about.iter().copied()).map(say).collect::<Vec<_>>().join(" "))
        .collect();
    let found = ranking::containing(&texts, query);
    let mut rows = Vec::new();
    for (group, members) in GROUPS {
        let mut heading = say(group);
        for member in members {
            if found.iter().filter_map(|position| tabs.get(*position)).any(|tab| tab.id == member.id) {
                rows.push(TabRow {
                    id: member.id.into(),
                    title: say(member.title).into(),
                    icon: member.icon.into(),
                    heading: std::mem::take(&mut heading).into(),
                });
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests;
