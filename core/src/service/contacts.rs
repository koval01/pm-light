//! Contacts and their cards.

use super::{CARD_SIGNED, CONTACTS_PAGE, ContactEmail, Method, Result, RngCore, Service, Value, json, text, vcard};

impl Service {
    pub async fn create_contact(&self, name: &str, email: &str) -> Result<()> {
        let mut uid = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut uid);
        let uid = crate::hex(&uid);
        let data = vcard::contact_card(name, email, &format!("proton-mail-light-{uid}"));
        let signature = self.crypto.sign_with_user_key(&data)?;
        let body = json!({
            "Contacts": [{"Cards": [{"Type": CARD_SIGNED, "Data": data, "Signature": signature}]}],
            "Overwrite": 0,
            "Labels": 0,
        });
        self.client.batch(Method::POST, "/contacts/v4/contacts", body, "Could not save the contact").await
    }

    pub async fn delete_contact(&self, id: &str) -> Result<()> {
        let ids = [id.to_string()];
        self.batch("/contacts/v4/contacts/delete", &ids, "Could not delete the contact", json!({})).await
    }

    pub async fn contact_emails(&self) -> Result<Vec<ContactEmail>> {
        let mut out = Vec::new();
        for page in 0.. {
            let path = format!("/contacts/v4/contacts/emails?PageSize={CONTACTS_PAGE}&Page={page}");
            let reply: Value = self.client.get(&path).await?;
            let rows = reply.get("ContactEmails").and_then(Value::as_array).cloned().unwrap_or_default();
            let count = rows.len();
            out.extend(rows.iter().map(|c| ContactEmail {
                name: text(c, "Name"),
                email: text(c, "Email"),
                contact_id: text(c, "ContactID"),
            }));
            let total = reply
                .get("Total")
                .and_then(Value::as_u64)
                .map_or(out.len(), |t| usize::try_from(t).unwrap_or(usize::MAX));
            if count < CONTACTS_PAGE || out.len() >= total {
                break;
            }
        }
        Ok(out)
    }
}
