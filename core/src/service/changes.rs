//! Changing the mailbox: moving, marking, deleting and labelling.

use super::{Error, MessageChanges, Method, Result, Service, Value, api, json};

impl Service {
    pub(super) async fn batch(&self, path: &str, ids: &[String], what: &str, extra: Value) -> Result<()> {
        for chunk in ids.chunks(api::BATCH_SIZE) {
            let mut body = extra.clone();
            crate::mapping::put(&mut body, "IDs", json!(chunk));
            self.client.batch(Method::PUT, path, body, what).await?;
        }
        Ok(())
    }

    pub async fn patch(&self, ids: &[String], changes: &MessageChanges) -> Result<()> {
        if let Some(unread) = changes.unread {
            let path = if unread { "/mail/v4/messages/unread" } else { "/mail/v4/messages/read" };
            self.batch(path, ids, "Could not change read state", json!({})).await?;
        }
        if let Some(starred) = changes.starred {
            let path = if starred { "/mail/v4/messages/label" } else { "/mail/v4/messages/unlabel" };
            self.batch(path, ids, "Could not change star", json!({"LabelID": api::STARRED})).await?;
        }
        if let Some(folder) = &changes.folder {
            // A folder is exclusive, so Proton takes the message out of the old one.
            let body = json!({"LabelID": api::label_of(folder)});
            self.batch("/mail/v4/messages/label", ids, "Could not move message", body).await?;
        }
        Ok(())
    }

    pub(super) fn are_conversations(&self, ids: &[String]) -> bool {
        ids.first().is_some_and(|id| self.is_conversation_row(id))
    }

    /// Rows may stand for conversations, whose whole selves then take the change, as in Proton's.
    pub(super) async fn change_rows(
        &self,
        ids: &[String],
        changes: &MessageChanges,
        conversations: bool,
    ) -> Result<()> {
        if !conversations {
            return self.patch(ids, changes).await;
        }
        let here = api::label_of(&self.view.lock().folder).to_string();
        if let Some(unread) = changes.unread {
            let (path, body) = if unread {
                ("/mail/v4/conversations/unread", json!({"LabelID": here}))
            } else {
                ("/mail/v4/conversations/read", json!({}))
            };
            self.batch(path, ids, "Could not change read state", body).await?;
        }
        if let Some(starred) = changes.starred {
            let path = if starred { "/mail/v4/conversations/label" } else { "/mail/v4/conversations/unlabel" };
            self.batch(path, ids, "Could not change star", json!({"LabelID": api::STARRED})).await?;
        }
        if let Some(folder) = &changes.folder {
            let body = json!({"LabelID": api::label_of(folder)});
            self.batch("/mail/v4/conversations/label", ids, "Could not move conversation", body).await?;
        }
        Ok(())
    }

    pub(super) async fn label_rows(&self, ids: &[String], label_id: &str, on: bool) -> Result<()> {
        if !self.are_conversations(ids) {
            return self.set_label(ids, label_id, on).await;
        }
        let path = if on { "/mail/v4/conversations/label" } else { "/mail/v4/conversations/unlabel" };
        self.batch(path, ids, "Could not change label", json!({"LabelID": label_id})).await
    }

    pub(super) async fn delete_rows(&self, ids: &[String], folder: &str, conversations: bool) -> Result<()> {
        if !conversations {
            return self.delete_permanently(ids, folder).await;
        }
        let body = json!({"LabelID": api::label_of(folder)});
        self.batch("/mail/v4/conversations/delete", ids, "Could not delete conversation", body).await
    }

    pub async fn set_label(&self, ids: &[String], label_id: &str, on: bool) -> Result<()> {
        let path = if on { "/mail/v4/messages/label" } else { "/mail/v4/messages/unlabel" };
        self.batch(path, ids, "Could not change label", json!({"LabelID": label_id})).await
    }

    pub async fn delete_permanently(&self, ids: &[String], folder: &str) -> Result<()> {
        let body = json!({"CurrentLabelID": api::label_of(folder)});
        self.batch("/mail/v4/messages/delete", ids, "Could not delete", body).await
    }

    pub async fn empty_folder(&self, folder: &str) -> Result<()> {
        if folder != "trash" && folder != "spam" {
            return Err(Error::other("Only Trash and Spam can be emptied"));
        }
        let path = format!("/mail/v4/messages/empty?LabelID={}", api::label_of(folder));
        self.client.act(Method::DELETE, &path).await
    }

    pub async fn create_label(&self, name: &str, kind: &str) -> Result<()> {
        let kind = if kind == "folder" { api::LABEL_TYPE_FOLDER } else { api::LABEL_TYPE_LABEL };
        let body = json!({"Name": name, "Color": api::DEFAULT_COLOR, "Type": kind});
        self.client.act_with(Method::POST, "/core/v4/labels", body).await
    }

    pub async fn rename_label(&self, id: &str, name: &str, color: &str) -> Result<()> {
        let body = json!({"Name": name, "Color": color});
        self.client.act_with(Method::PUT, &format!("/core/v4/labels/{id}"), body).await
    }

    pub async fn delete_label(&self, id: &str) -> Result<()> {
        self.client.act(Method::DELETE, &format!("/core/v4/labels/{id}")).await
    }
}
