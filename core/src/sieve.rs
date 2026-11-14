//! Turns the simple filter form into the script Proton runs.

use serde::Deserialize;

pub mod highlight;

const REQUIRE_ACTIONS: &str = r#"require ["include", "environment", "variables", "relational", "comparator-i;ascii-numeric", "spamtest"];
require ["fileinto", "imap4flags"];

# Generated: Do not run this script on spam messages
if allof (environment :matches "vnd.proton.spam-threshold" "*", spamtest :value "ge" :comparator "i;ascii-numeric" "${1}") {
    return;
}
"#;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleFilter {
    pub field: String,
    pub value: String,
    /// Folder or label name, or a system folder such as "archive".
    pub target: Option<String>,
    pub mark_read: bool,
    pub star: bool,
}

/// Sieve strings escape backslashes and quotes; "${" would start a variable.
fn quoted(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"").replace("${", "${dollar}{");
    format!("\"{escaped}\"")
}

fn test_for(field: &str) -> &'static str {
    match field {
        "subject" => r#"header :comparator "i;unicode-casemap" :contains "Subject""#,
        "to" => r#"address :all :comparator "i;unicode-casemap" :contains ["To", "Cc", "Bcc"]"#,
        _ => r#"address :all :comparator "i;unicode-casemap" :contains "From""#,
    }
}

#[must_use]
pub fn build_sieve(filter: &SimpleFilter) -> String {
    let mut actions = Vec::new();
    if filter.mark_read {
        actions.push(r#"    addflag "\\Seen";"#.to_string());
    }
    if filter.star {
        actions.push(r#"    addflag "\\Flagged";"#.to_string());
    }
    if let Some(target) = filter.target.as_deref().filter(|t| !t.is_empty()) {
        actions.push(format!("    fileinto {};", quoted(target)));
    }
    let dollar = if filter.value.contains("${") { "require \"variables\";\nset \"dollar\" \"$\";\n" } else { "" };
    format!(
        "{REQUIRE_ACTIONS}\n{dollar}if {} {} {{\n{}\n}}\n",
        test_for(&filter.field),
        quoted(&filter.value),
        actions.join("\n"),
    )
}

#[cfg(test)]
mod tests;
