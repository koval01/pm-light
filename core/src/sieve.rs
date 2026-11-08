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
mod tests {
    use super::*;

    fn filter(field: &str, value: &str, target: Option<&str>) -> SimpleFilter {
        SimpleFilter {
            field: field.into(),
            value: value.into(),
            target: target.map(str::to_string),
            mark_read: false,
            star: false,
        }
    }

    #[test]
    fn declares_what_it_uses_and_skips_spam_like_proton_scripts() {
        let sieve = build_sieve(&filter("subject", "café", Some("archive")));
        assert!(sieve.contains(r#"require ["fileinto", "imap4flags"];"#));
        assert!(sieve.contains("# Generated: Do not run this script on spam messages"));
        assert!(sieve.contains(r#""${1}""#));
    }

    #[test]
    fn matches_the_right_field() {
        let subject = build_sieve(&filter("subject", "café", Some("archive")));
        assert!(subject.contains(r#"if header :comparator "i;unicode-casemap" :contains "Subject" "café" {"#));
        assert!(build_sieve(&filter("from", "x", Some("y"))).contains(r#":contains "From" "x""#));
        assert!(build_sieve(&filter("to", "x", Some("y"))).contains(r#"["To", "Cc", "Bcc"]"#));
    }

    #[test]
    fn files_into_the_target() {
        assert!(build_sieve(&filter("subject", "x", Some("Café"))).contains(r#"fileinto "Café";"#));
    }

    #[test]
    fn sets_flags_with_an_escaped_backslash_and_files_nothing() {
        let f = SimpleFilter { mark_read: true, star: true, ..filter("subject", "x", None) };
        let sieve = build_sieve(&f);
        assert!(sieve.contains(r#"addflag "\\Seen";"#) && sieve.contains(r#"addflag "\\Flagged";"#));
        assert!(!sieve.contains("fileinto \""));
    }

    #[test]
    fn an_empty_target_files_nothing() {
        assert!(!build_sieve(&filter("subject", "x", Some(""))).contains("fileinto \""));
    }

    #[test]
    fn cannot_be_broken_out_of_with_quotes() {
        let sieve = build_sieve(&filter("subject", r#"a" { stop; } ""#, Some("x")));
        assert!(sieve.contains(r#""a\" { stop; } \"""#));
    }

    #[test]
    fn escapes_backslashes() {
        assert!(build_sieve(&filter("subject", r"a\b", Some("x"))).contains(r#""a\\b""#));
    }

    #[test]
    fn keeps_a_literal_dollar_brace_from_becoming_a_variable() {
        let sieve = build_sieve(&filter("subject", "${x}", Some("y")));
        assert!(sieve.contains(r#"set "dollar" "$";"#));
        assert!(sieve.contains(r#""${dollar}{x}""#));
    }
}
