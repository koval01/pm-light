//! What a row of the view opens: itself when it is a message, and one of its messages when it is a conversation,
//! which only the view can tell while the row is in it.

use super::{Error, Result, Service, api};

/// A row as the view held it when asked: whether it was there at all, and the folder it was shown in when it is a
/// conversation's, which chooses its message.
#[derive(Debug, PartialEq, Eq)]
pub struct RowOpens {
    pub(super) id: String,
    pub(super) conversation_in: Option<String>,
    pub(super) in_view: bool,
}

impl RowOpens {
    /// A row the view did not hold is no row to read ahead: what it is cannot be told.
    #[must_use]
    pub const fn in_view(&self) -> bool {
        self.in_view
    }
}

impl Service {
    /// The message a row opens. Only a conversation has to ask, and it asks once.
    pub async fn message_of_row(&self, id: &str) -> Result<String> {
        let row = self.row_opens(id);
        self.message_of(row).await
    }

    /// What a row opens, settled while the row is in the view. Read later, after the person went to another folder,
    /// a conversation's row was taken for a message, and its id was asked for as one, which Proton refuses.
    pub fn row_opens(&self, id: &str) -> RowOpens {
        let (in_view, conversation, folder) = {
            let view = self.view.lock();
            let row = view.row(id);
            (row.is_some(), row.is_some_and(|row| row.messages > 0), view.folder.clone())
        };
        RowOpens { id: id.to_string(), conversation_in: conversation.then_some(folder), in_view }
    }

    /// The message a row settled by `row_opens` opens. A row the view did not hold is taken for a message, as a mail
    /// chosen inside a conversation is.
    pub async fn message_of(&self, row: RowOpens) -> Result<String> {
        let Some(folder) = row.conversation_in else { return Ok(row.id) };
        let thread = self.thread(&row.id).await?;
        api::message_to_open(&thread, &folder).ok_or_else(|| Error::other("An empty conversation"))
    }

    pub fn is_conversation_row(&self, id: &str) -> bool {
        self.view.lock().row(id).is_some_and(|row| row.messages > 0)
    }

    pub fn has_row(&self, id: &str) -> bool {
        self.view.lock().row(id).is_some()
    }
}
