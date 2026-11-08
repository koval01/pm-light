//! The same letters as `compose_from`, written for a `text/plain` draft.
//!
//! Proton's own client writes them so: four empty lines to type on, the signature, and what is answered with `> ` before each line. Nothing here goes
//! through the editor's cleaning, because nothing here is markup.

use super::{ComposeLabels, ComposeMode, ComposeState, emails, signature_for};
use crate::{
    model::{Message, OwnAddress},
    text::to_text,
};

const ROOM_TO_WRITE: &str = "\n\n\n\n";
const QUOTE: &str = "> ";

fn text_of(m: &Message) -> String {
    if m.html { to_text(&m.safe_body) } else { m.body.replace("\r\n", "\n") }
}

fn quoted(text: &str) -> String {
    text.split('\n').map(|line| format!("{QUOTE}{line}")).collect::<Vec<_>>().join("\n")
}

fn sender(m: &Message) -> String {
    let from = &m.meta.from;
    from.name.as_ref().map_or_else(|| from.email.clone(), |name| format!("{name} <{}>", from.email))
}

fn replied(m: &Message, labels: &ComposeLabels) -> String {
    format!("{}, {}:\n\n{}", labels.date_time, sender(m), quoted(&text_of(m)))
}

fn forwarded(m: &Message, labels: &ComposeLabels) -> String {
    let header = [
        format!("---------- {} ----------", labels.forwarded),
        format!("{}: {}", labels.from, sender(m)),
        format!("{}: {}", labels.date, labels.date_time),
        format!("{}: {}", labels.subject, m.meta.subject),
        format!("{}: {}", labels.to, emails(&m.meta.to).join(", ")),
    ]
    .join("\n");
    format!("{header}\n\n{}", quoted(&text_of(m)))
}

fn opening(signature: &str, rest: &str) -> String {
    if signature.is_empty() {
        format!("{ROOM_TO_WRITE}{rest}")
    } else {
        format!("{ROOM_TO_WRITE}{signature}\n\n{rest}")
    }
}

/// Only the body differs: who it goes to and what it answers are worked out the same way.
#[must_use]
pub fn into_plain(
    state: ComposeState,
    mode: ComposeMode,
    message: Option<&Message>,
    own: &[OwnAddress],
    labels: &ComposeLabels,
) -> ComposeState {
    let signature = to_text(signature_for(own, state.from.as_deref()));
    let body = match (mode, message) {
        (ComposeMode::Draft, Some(m)) => text_of(m),
        (ComposeMode::Forward, Some(m)) => opening(&signature, &forwarded(m, labels)),
        (ComposeMode::Reply | ComposeMode::ReplyAll, Some(m)) => opening(&signature, &replied(m, labels)),
        _ => opening(&signature, ""),
    };
    ComposeState { body, plain: true, ..state }
}

/// A letter a link asked for, which has no signature in rich text either.
#[must_use]
pub fn prepared(state: ComposeState) -> ComposeState {
    ComposeState { body: to_text(&state.body), plain: true, ..state }
}

/// As in `replace_signature`, for text. With none before, the new one goes to the end, as in Proton's.
#[must_use]
pub fn replace_signature(body: &str, old: &str, new: &str) -> String {
    let (old, new) = (to_text(old), to_text(new));
    if old.is_empty() {
        return if new.is_empty() { body.to_string() } else { format!("{body}\n\n{new}") };
    }
    body.replacen(&old, &new, 1).trim_end().to_string()
}

#[cfg(test)]
mod tests;
