//! The one error type of the core: what went wrong, how the connection to Proton stands, and the words a person
//! sees for it. Its kind, never its text, is what the journal keeps.

use serde::{Serialize, Serializer, ser::SerializeStruct};
use std::time::Duration;

pub type Result<T> = std::result::Result<T, Error>;

/// How the connection to Proton stands. Neither way of being away says anything about the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Link {
    Online,
    /// Nothing answered.
    Offline,
    /// Proton answered, but cannot serve right now.
    Unavailable,
    /// Something answered with a key that is not Proton's or Quad9's.
    Untrusted,
}

impl Link {
    /// Trying again is cheap while nothing answers, and pointless at a struggling or false server.
    #[must_use]
    pub fn retry_delay(self) -> Duration {
        Duration::from_secs(if self == Self::Offline { 5 } else { 30 })
    }
}

/// A page a person must complete on Proton's site. This client never attempts it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanVerification {
    pub token: String,
    pub methods: Vec<String>,
    pub web_url: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{message}")]
    Api { status: u16, code: i64, message: String },
    #[error("human verification required")]
    Verification(HumanVerification),
    #[error("cannot reach Proton: {0}")]
    Network(String),
    #[error("the connection failed its security check")]
    Untrusted,
    #[error("wrong password")]
    WrongPassword,
    #[error("too many attempts, wait {seconds} s")]
    TooManyAttempts { seconds: u64 },
    #[error("cancelled")]
    Cancelled,
    #[error("the session has ended")]
    SessionEnded,
    #[error("Proton refuses this move")]
    CannotMove,
    #[error("too many messages to print at once")]
    TooManyToPrint { limit: usize },
    #[error("this message is too long to seal")]
    TooLongToSeal,
    #[error("this file is not a readable scan")]
    UnreadableScan,
    #[error("not a time")]
    BadTime,
    #[error("that time has passed")]
    PastTime,
    #[error("Not a valid address: {0}")]
    InvalidAddress(String),
    #[error("{0}")]
    Crypto(String),
    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other(message.into())
    }

    pub fn other_from(error: impl std::fmt::Display) -> Self {
        Self::Other(error.to_string())
    }

    pub fn crypto(error: impl std::fmt::Display) -> Self {
        Self::Crypto(error.to_string())
    }

    /// What this error says about the connection. `None` means it was answered, so it is not a
    /// connection problem.
    #[must_use]
    pub const fn link(&self) -> Option<Link> {
        match self {
            Self::Network(_) => Some(Link::Offline),
            Self::Untrusted => Some(Link::Untrusted),
            Self::Api { status: 409 | 429 | 500..=599, .. } => Some(Link::Unavailable),
            _ => None,
        }
    }

    /// The name of this error, safe for the journal: a server's message may repeat an address.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Api { .. } => "api",
            Self::Verification(_) => "verification",
            Self::Network(_) => "network",
            Self::Untrusted => "untrusted",
            Self::InvalidAddress(_) => "invalidAddress",
            Self::WrongPassword => "wrongPassword",
            Self::TooManyAttempts { .. } => "tooManyAttempts",
            Self::Cancelled => "cancelled",
            Self::SessionEnded => "sessionEnded",
            Self::CannotMove => "cannotMove",
            Self::TooManyToPrint { .. } => "tooManyToPrint",
            Self::TooLongToSeal => "tooLongToSeal",
            Self::UnreadableScan => "unreadableScan",
            Self::BadTime => "badTime",
            Self::PastTime => "pastTime",
            Self::Crypto(_) => "crypto",
            Self::Other(_) => "other",
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        if crate::tls::is_refused_certificate(&error) {
            log::error!("a connection failed its certificate check");
            return Self::Untrusted;
        }
        // The address may hold a mail's image URL, and errors reach logs and the screen.
        Self::Network(error.without_url().to_string())
    }
}

/// Only the kind: an I/O error's text can name a path.
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Network(format!("{:?}", error.kind()))
    }
}

/// What the interface receives: enough to show the error, nothing it would have to interpret.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("Error", 7)?;
        out.serialize_field("kind", self.kind())?;
        out.serialize_field("message", &self.to_string())?;
        if let Some(link) = self.link() {
            out.serialize_field("link", &link)?;
        }
        if let Self::Api { status, code, .. } = self {
            out.serialize_field("status", status)?;
            out.serialize_field("code", code)?;
        }
        if let Self::InvalidAddress(address) = self {
            out.serialize_field("address", address)?;
        }
        if let Self::TooManyToPrint { limit } = self {
            out.serialize_field("limit", limit)?;
        }
        if let Self::TooManyAttempts { seconds } = self {
            out.serialize_field("retryAfter", seconds)?;
        }
        out.end()
    }
}
