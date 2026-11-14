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
