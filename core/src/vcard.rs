//! A contact as a vCard, the way Proton keeps one: a sender's name and address made safe to stand in its lines.

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
mod tests;
