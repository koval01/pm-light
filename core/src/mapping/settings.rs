//! Mail settings, auto-reply and filter issues between Proton's shape and ours.

use super::{
    ApiUnsubscribe, AutoReply, Habits, MailSettings, MailtoUnsubscribe, REPEAT_FIXED, REPEAT_PERMANENT,
    UnsubscribeMethods, UserChoices, Value, json, plaintext_to_html, to_text,
};

/// Endpoint and values of each toggle, from Proton's web client.
pub struct Toggle {
    pub key: &'static str,
    pub path: &'static str,
    pub field: &'static str,
    /// Where the server's answer differs from what it is sent.
    pub read: Option<&'static str>,
    pub on: u64,
    pub off: u64,
}

pub const TOGGLES: &[Toggle] = &[
    Toggle {
        key: "hideRemoteImages",
        path: "hide-remote-images",
        field: "HideRemoteImages",
        read: None,
        on: 1,
        off: 0,
    },
    Toggle {
        key: "nextMessageOnMove",
        path: "next-message-on-move",
        field: "NextMessageOnMove",
        read: None,
        on: 2,
        off: 0,
    },
    Toggle { key: "shortcuts", path: "shortcuts", field: "Shortcuts", read: None, on: 1, off: 0 },
    Toggle { key: "autoSaveContacts", path: "autocontacts", field: "AutoSaveContacts", read: None, on: 1, off: 0 },
    Toggle { key: "confirmLinks", path: "confirmlink", field: "ConfirmLink", read: None, on: 1, off: 0 },
    Toggle {
        key: "autoDeleteOld",
        path: "auto-delete-spam-and-trash-days",
        field: "Days",
        read: Some("AutoDeleteSpamAndTrashDays"),
        on: 30,
        off: 0,
    },
];

/// One value a setting may take. Nothing else ever goes to the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Allowed {
    Number(i64),
    Text(&'static str),
    Flag(bool),
    Unset,
}

impl Allowed {
    fn to_json(self) -> Value {
        match self {
            Self::Number(number) => Value::from(number),
            Self::Text(text) => Value::from(text),
            Self::Flag(flag) => Value::from(flag),
            Self::Unset => Value::Null,
        }
    }

    fn matches(self, value: Option<&Value>) -> bool {
        match (self, value) {
            (Self::Unset, None | Some(Value::Null)) => true,
            (Self::Number(number), Some(value)) => value.as_i64() == Some(number),
            (Self::Text(text), Some(value)) => value.as_str() == Some(text),
            (Self::Flag(flag), Some(value)) => value.as_bool() == Some(flag) || value.as_u64() == Some(u64::from(flag)),
            _ => false,
        }
    }
}

/// A setting of the mail, read and written by name. Values follow `mailSettings.ts` in Proton's client.
#[derive(Clone, Copy, Debug)]
pub struct MailChoice {
    pub key: &'static str,
    pub path: &'static str,
    /// As it is sent.
    pub field: &'static str,
    /// As it comes back, which can differ.
    pub read: &'static str,
    pub values: &'static [Allowed],
}

const OFF_ON: &[Allowed] = &[Allowed::Number(0), Allowed::Number(1)];

const fn choice(key: &'static str, path: &'static str, field: &'static str, values: &'static [Allowed]) -> MailChoice {
    MailChoice { key, path, field, read: field, values }
}

pub const FONT_FACES: [&str; 10] = [
    "Sans-serif",
    "Serif",
    "Georgia",
    "Arial",
    "Helvetica",
    "Menlo, Consolas, Courier New, Monospace",
    "Tahoma, sans-serif",
    "Verdana",
    "Times New Roman",
    "Trebuchet MS",
];
pub const FONT_SIZES: [i64; 8] = [10, 12, 14, 16, 18, 20, 22, 24];
/// Proton's list stops here, whatever the setting says.
const MAX_LIST_PAGE: u64 = 150;
const MOVED_IN_DRAFTS: i64 = 1;
/// `VIEW_MODE.GROUP`: mail stands in conversations.
const VIEW_GROUPED: i64 = 0;
const FOOTER_ENABLED: i64 = 1;
/// The plan decides, and the account cannot change it.
const FOOTER_LOCKED: i64 = 2;
const MOVED_IN_SENT: i64 = 2;

pub const MAIL_CHOICES: &[MailChoice] = &[
    choice(
        "showMoved",
        "moved",
        "ShowMoved",
        &[Allowed::Number(0), Allowed::Number(1), Allowed::Number(2), Allowed::Number(3)],
    ),
    choice("viewMode", "viewmode", "ViewMode", OFF_ON),
    choice("viewLayout", "viewlayout", "ViewLayout", OFF_ON),
    choice("composerMode", "composermode", "ComposerMode", OFF_ON),
    choice("pageSize", "pagesize", "PageSize", &[Allowed::Number(50), Allowed::Number(100), Allowed::Number(200)]),
    choice("stickyLabels", "stickylabels", "StickyLabels", OFF_ON),
    choice("almostAllMail", "almost-all-mail", "AlmostAllMail", OFF_ON),
    choice("protonFooter", "pmsignature", "PMSignature", OFF_ON),
    MailChoice {
        key: "draftType",
        path: "drafttype",
        field: "MIMEType",
        read: "DraftMIMEType",
        values: &[Allowed::Text("text/html"), Allowed::Text("text/plain")],
    },
    choice("direction", "righttoleft", "RightToLeft", OFF_ON),
    choice("hideEmbedded", "hide-embedded-images", "HideEmbeddedImages", OFF_ON),
    choice("hideSenderImages", "hide-sender-images", "HideSenderImages", OFF_ON),
    choice(
        "removeMetadata",
        "remove-image-metadata",
        "RemoveImageMetadata",
        &[Allowed::Flag(false), Allowed::Flag(true)],
    ),
    choice(
        "blockConfirmation",
        "block-sender-confirmation",
        "BlockSenderConfirmation",
        &[Allowed::Unset, Allowed::Number(1)],
    ),
    choice("spamAction", "spam-action", "SpamAction", &[Allowed::Unset, Allowed::Number(0), Allowed::Number(1)]),
    choice("sign", "sign", "Sign", OFF_ON),
    choice("attachPublicKey", "attachpublic", "AttachPublicKey", OFF_ON),
    choice("pgpScheme", "pgpscheme", "PGPScheme", &[Allowed::Number(16), Allowed::Number(8)]),
    choice("promptPin", "promptpin", "PromptPin", OFF_ON),
    choice("folderColor", "enablefoldercolor", "EnableFolderColor", OFF_ON),
    choice("inheritFolderColor", "inheritparentfoldercolor", "InheritParentFolderColor", OFF_ON),
    choice(
        "fontFace",
        "fontface",
        "FontFace",
        &[
            Allowed::Text(FONT_FACES[0]),
            Allowed::Text(FONT_FACES[1]),
            Allowed::Text(FONT_FACES[2]),
            Allowed::Text(FONT_FACES[3]),
            Allowed::Text(FONT_FACES[4]),
            Allowed::Text(FONT_FACES[5]),
            Allowed::Text(FONT_FACES[6]),
            Allowed::Text(FONT_FACES[7]),
            Allowed::Text(FONT_FACES[8]),
            Allowed::Text(FONT_FACES[9]),
        ],
    ),
    choice(
        "fontSize",
        "fontsize",
        "FontSize",
        &[
            Allowed::Number(FONT_SIZES[0]),
            Allowed::Number(FONT_SIZES[1]),
            Allowed::Number(FONT_SIZES[2]),
            Allowed::Number(FONT_SIZES[3]),
            Allowed::Number(FONT_SIZES[4]),
            Allowed::Number(FONT_SIZES[5]),
            Allowed::Number(FONT_SIZES[6]),
            Allowed::Number(FONT_SIZES[7]),
        ],
    ),
];

/// Which of its values each setting stands at, where it stands at one of them.
#[must_use]
pub fn read_choices(raw: &Value) -> std::collections::HashMap<&'static str, usize> {
    MAIL_CHOICES
        .iter()
        .filter_map(|choice| {
            choice
                .values
                .iter()
                .position(|value| value.matches(raw.get(choice.read)))
                .map(|position| (choice.key, position))
        })
        .collect()
}

/// What the client has to know of the settings. Anything unknown counts as not set, and the font
/// comes only from the table: it goes into the letter's markup.
pub fn to_habits(raw: &Value) -> Habits {
    let number = |name: &str| raw.get(name).and_then(Value::as_i64).unwrap_or(0);
    let shown = number("ShowMoved");
    let page = u64::try_from(number("PageSize")).ok().filter(|page| (1..=MAX_LIST_PAGE).contains(page));
    Habits {
        page_size: page.unwrap_or_else(|| if number("PageSize") > 0 { MAX_LIST_PAGE } else { crate::view::PAGE }),
        moved_in_drafts: shown & MOVED_IN_DRAFTS != 0,
        moved_in_sent: shown & MOVED_IN_SENT != 0,
        almost_all_mail: number("AlmostAllMail") == 1,
        folder_colors: number("EnableFolderColor") == 1,
        wide_composer: number("ComposerMode") == 1,
        plain_drafts: raw.get("DraftMIMEType").and_then(Value::as_str) == Some("text/plain"),
        right_to_left: number("RightToLeft") == 1,
        font_face: raw
            .get("FontFace")
            .and_then(Value::as_str)
            .and_then(|face| FONT_FACES.iter().find(|known| **known == face))
            .map(ToString::to_string),
        font_size: Some(number("FontSize")).filter(|size| FONT_SIZES.contains(size)),
        skip_block_question: number("BlockSenderConfirmation") == 1,
        unsubscribe_on_spam: number("SpamAction") == 1,
        conversations: raw.get("ViewMode").and_then(Value::as_i64) == Some(VIEW_GROUPED),
        hide_embedded: number("HideEmbeddedImages") == 1,
        remove_metadata: raw.get("RemoveImageMetadata").and_then(Value::as_bool).unwrap_or(false),
        proton_footer: number("PMSignature") & FOOTER_ENABLED != 0,
        proton_footer_locked: number("PMSignature") & FOOTER_LOCKED != 0,
    }
}

/// Where a change goes and what it says, or nothing for what is not in the table.
#[must_use]
pub fn choice_change(key: &str, position: usize) -> Option<(String, Value)> {
    let choice = MAIL_CHOICES.iter().find(|choice| choice.key == key)?;
    let value = choice.values.get(position)?;
    Some((format!("/mail/v4/settings/{}", choice.path), json!({choice.field: value.to_json()})))
}

/// Subscriptions from Proton: the name it takes and its bit in `UserSettings.News`.
pub const NEWS: [(&str, u32); 13] = [
    ("Announcements", 0),
    ("Newsletter", 2),
    ("Beta", 3),
    ("Business", 4),
    ("Offers", 5),
    ("NewEmailNotif", 6),
    ("Onboarding", 7),
    ("UserSurveys", 8),
    ("InboxNews", 9),
    ("VpnNews", 10),
    ("DriveNews", 11),
    ("PassNews", 12),
    ("AccountRecovery", 17),
];

/// Account settings that are a plain yes or no.
pub const USER_FLAGS: [(&str, &str, &str); 2] =
    [("telemetry", "telemetry", "Telemetry"), ("crashReports", "crashreports", "CrashReports")];

pub fn to_user_choices(settings: &Value) -> UserChoices {
    let flag = |pointer: &str| settings.pointer(pointer).and_then(Value::as_u64).unwrap_or(0) != 0;
    UserChoices {
        telemetry: flag("/Telemetry"),
        crash_reports: flag("/CrashReports"),
        news: settings.get("News").and_then(Value::as_u64).unwrap_or(0),
        breach_alerts_offered: flag("/BreachAlerts/Eligible"),
        breach_alerts: flag("/BreachAlerts/Value"),
        sentinel_offered: flag("/HighSecurity/Eligible"),
        sentinel: flag("/HighSecurity/Value"),
        invoice_text: settings.get("InvoiceText").and_then(Value::as_str).unwrap_or_default().to_string(),
    }
}

#[must_use]
pub fn toggle(key: &str) -> Option<&'static Toggle> {
    TOGGLES.iter().find(|t| t.key == key)
}

pub fn to_mail_settings(raw: &Value) -> MailSettings {
    let read = |key: &str| {
        toggle(key)
            .and_then(|t| raw.get(t.read.unwrap_or(t.field)))
            .is_some_and(|value| value.as_u64().unwrap_or(0) != 0 || value.as_bool() == Some(true))
    };
    MailSettings {
        delay_send_seconds: raw.get("DelaySendSeconds").and_then(Value::as_u64).unwrap_or(0),
        hide_remote_images: read("hideRemoteImages"),
        next_message_on_move: read("nextMessageOnMove"),
        shortcuts: read("shortcuts"),
        auto_save_contacts: read("autoSaveContacts"),
        confirm_links: read("confirmLinks"),
        auto_delete_old: read("autoDeleteOld"),
    }
}

/// The full address order with `id` first: Proton's default address is the first one.
#[must_use]
pub fn with_default_first(ids: &[String], id: &str) -> Vec<String> {
    std::iter::once(id.to_string()).chain(ids.iter().filter(|x| *x != id).cloned()).collect()
}

#[must_use]
pub const fn is_auto_forwarded(flags: u64) -> bool {
    flags & (1 << 35) != 0
}

#[must_use]
pub fn to_unsubscribe(raw: Option<&ApiUnsubscribe>) -> Option<UnsubscribeMethods> {
    let raw = raw.filter(|r| r.one_click.is_some() || r.http_client.is_some() || r.mailto.is_some())?;
    Some(UnsubscribeMethods {
        one_click: raw.one_click.is_some(),
        url: raw.http_client.clone(),
        mailto: raw.mailto.as_ref().map(|m| MailtoUnsubscribe {
            to: m.to_list.clone(),
            subject: m.subject.clone().unwrap_or_else(|| "Unsubscribe".into()),
            body: m.body.clone().unwrap_or_else(|| "Please, unsubscribe me".into()),
        }),
    })
}

pub fn to_auto_reply(raw: Option<&Value>) -> AutoReply {
    let text = |name: &str| raw.and_then(|r| r.get(name)).and_then(Value::as_str).unwrap_or_default();
    let number = |name: &str| raw.and_then(|r| r.get(name)).and_then(Value::as_u64).unwrap_or(0);
    let subject = text("Subject");
    AutoReply {
        enabled: raw.and_then(|r| r.get("IsEnabled")).and_then(Value::as_bool).unwrap_or(false),
        permanent: number("Repeat") == REPEAT_PERMANENT,
        start: number("StartTime"),
        end: number("EndTime"),
        subject: if subject.is_empty() { "Auto".into() } else { subject.into() },
        text: to_text(text("Message")),
    }
}

#[must_use]
pub fn from_auto_reply(reply: &AutoReply, zone: &str) -> Value {
    json!({
        "StartTime": if reply.permanent { 0 } else { reply.start },
        "EndTime": if reply.permanent { 0 } else { reply.end },
        "Repeat": if reply.permanent { REPEAT_PERMANENT } else { REPEAT_FIXED },
        "DaysSelected": [],
        "Subject": reply.subject,
        "Message": plaintext_to_html(&reply.text),
        "IsEnabled": reply.enabled,
        "Zone": zone,
    })
}

/// Proton counts lines from zero.
pub fn issue_text(issue: &Value) -> String {
    let message = issue.get("message").and_then(Value::as_str).unwrap_or("Invalid filter");
    issue
        .pointer("/from/line")
        .and_then(Value::as_u64)
        .map_or_else(|| message.to_string(), |line| format!("{}: {message}", line + 1))
}
