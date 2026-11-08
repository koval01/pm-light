fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace("\r\n", "\\n")
        .replace(['\r', '\n'], "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

/// The signed card Proton stores for a contact: name and address only, both visible to the server.
#[must_use]
pub fn contact_card(name: &str, email: &str, uid: &str) -> String {
    let lines = [
        "BEGIN:VCARD".to_string(),
        "VERSION:4.0".to_string(),
        "PRODID:-//proton-mail-light//EN".to_string(),
        format!("FN:{}", escape(name)),
        format!("item1.EMAIL;PREF=1:{}", email.chars().filter(|c| !c.is_control()).collect::<String>().trim()),
        format!("UID:{uid}"),
        "END:VCARD".to_string(),
        String::new(),
    ];
    lines.join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::contact_card;

    #[test]
    fn is_a_vcard_4_with_crlf_endings() {
        let card = contact_card("A", "a@x.io", "u");
        assert!(card.starts_with("BEGIN:VCARD\r\nVERSION:4.0\r\n"));
        assert!(card.ends_with("END:VCARD\r\n"));
    }

    /// A display name comes from the sender, who could add a line of their own to the card.
    #[test]
    fn a_name_or_address_cannot_add_a_line() {
        let card = contact_card("A\r\nEMAIL:evil@x.io\rNOTE:x", "a@x.io\r\nURL:https://evil", "u");
        assert_eq!(card.lines().count(), 7, "{card}");
        assert!(card.lines().all(|line| !line.starts_with("EMAIL:") && !line.starts_with("URL:")));
    }

    #[test]
    fn escapes_the_name_and_trims_the_address() {
        let card = contact_card("Ada, Countess; of \\ Lovelace", " ada@x.io ", "u");
        assert!(card.contains("FN:Ada\\, Countess\\; of \\\\ Lovelace"));
        assert!(card.contains("item1.EMAIL;PREF=1:ada@x.io\r\n"));
    }
}
