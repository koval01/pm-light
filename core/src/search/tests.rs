use super::*;
use crate::model::Address;

fn row(subject: &str, from: &str, files: u64) -> MessageMeta {
    MessageMeta {
        id: "m".into(),
        folder: "inbox".into(),
        from: Address { name: Some("Ann Lee".into()), email: from.into() },
        to: vec![Address { name: None, email: "me@example.org".into() }],
        subject: subject.into(),
        snippet: "a short preview".into(),
        date: 0,
        unread: false,
        starred: false,
        labels: vec![],
        attachment_count: files,
        conversation_id: String::new(),
        messages: 0,
        expires: 0,
    }
}

fn joined(parsed: &Parsed) -> String {
    parsed.spans.iter().map(|span| span.text.as_str()).collect()
}

fn parts(input: &str) -> Vec<Part> {
    parse(input).spans.iter().filter(|span| span.part != Part::Space).map(|span| span.part).collect()
}

#[test]
fn plain_words_stay_a_keyword() {
    let parsed = parse("hello world");
    assert_eq!(parsed.query.keyword_text().as_deref(), Some("hello world"));
    assert_eq!(parsed.problem, None);
    assert!(!parsed.query.checks_here());
}

#[test]
fn operators_go_to_the_server() {
    let parsed = parse(r#"from:ann to:bob subject:"big news" has:attachment is:unread is:starred"#);
    let query = parsed.query;
    assert_eq!(query.from.as_deref(), Some("ann"));
    assert_eq!(query.to.as_deref(), Some("bob"));
    assert_eq!(query.subject.as_deref(), Some("big news"));
    assert!(query.has_attachment);
    assert_eq!(query.unread, Some(true));
    assert_eq!(query.starred, Some(true));
    assert!(query.checks.is_empty());
    assert_eq!(parsed.problem, None);
}

#[test]
fn dates_are_read_in_both_forms() {
    let query = parse("after:2026-01-02 before:03.02.2026").query;
    assert_eq!(query.after, date_of("2026-01-02"));
    assert_eq!(query.before, date_of("2026-02-03"));
    assert!(query.after.is_some());
}

#[test]
fn an_end_excludes_the_day_named() {
    let query = parse("before:2026-02-03").query;
    let at = date_of("2026-02-03").unwrap_or(0);
    assert_eq!(begin_and_end(&query), (None, Some(at - 1)));
}

#[test]
fn the_text_can_always_be_rebuilt_from_its_pieces() {
    for input in [
        "",
        "  ",
        " a  b ",
        "-x",
        "- x",
        r#"from:"a b" -/x\/y/i"#,
        "foo:bar",
        "from:",
        "/open",
        "\"open",
        "\u{44f} \u{44e}",
    ] {
        assert_eq!(joined(&parse(input)), input, "{input}");
    }
}

#[test]
fn parts_are_told_apart_for_colour() {
    assert_eq!(parts("from:ann word -/x/i"), [Part::Key, Part::Value, Part::Word, Part::Negation, Part::Pattern]);
}

#[test]
fn mistakes_are_named() {
    assert_eq!(parse("from:").problem, Some(Problem::MissingValue("from".into())));
    assert_eq!(parse("before:yesterday").problem, Some(Problem::BadDate("yesterday".into())));
    assert_eq!(parse("is:lost").problem, Some(Problem::UnknownValue("lost".into())));
    assert_eq!(parse("has:cat").problem, Some(Problem::UnknownValue("cat".into())));
    assert_eq!(parse("\"open").problem, Some(Problem::OpenQuote));
    assert_eq!(parse("/open").problem, Some(Problem::OpenPattern));
    assert_eq!(parse("/(/").problem, Some(Problem::BadPattern));
    assert_eq!(parse("/a/x").problem, Some(Problem::BadPattern));
    assert_eq!(parse("fron:ann").problem, Some(Problem::DidYouMean("from".into())));
    assert_eq!(parse("after:2026-02-02 before:2026-02-01").problem, Some(Problem::WrongOrder));
    assert!(parts("before:yesterday").contains(&Part::Wrong));
}

#[test]
fn words_with_a_colon_are_only_words() {
    for input in ["10:30", "https://example.org", "note:this"] {
        assert_eq!(parse(input).problem, None, "{input}");
    }
}

#[test]
fn patterns_are_checked_here() {
    let query = parse(r"/invoice\s+\d+/i").query;
    assert!(query.keyword.is_empty());
    assert!(query.matches(&row("Your INVOICE 2231", "a@b.org", 0)));
    assert!(!query.matches(&row("Your invoice", "a@b.org", 0)));
    let sensitive = parse("/Invoice/").query;
    assert!(!sensitive.matches(&row("invoice", "a@b.org", 0)));
}

#[test]
fn a_pattern_can_look_at_one_field() {
    let query = parse(r"from:/@example\.(org|com)$/").query;
    assert!(query.matches(&row("x", "ann@example.org", 0)));
    assert!(!query.matches(&row("ann@example.org", "z@else.net", 0)));
    let subject = parse("subject:/^Re:/").query;
    assert!(subject.matches(&row("Re: x", "a@b.org", 0)));
    assert!(!subject.matches(&row("x Re:", "a@b.org", 0)));
}

#[test]
fn exclusions_are_checked_here() {
    let query = parse("-spam -from:ann@b.org -has:attachment").query;
    assert_eq!(query.from, None);
    assert!(query.matches(&row("hello", "other@b.org", 0)));
    assert!(!query.matches(&row("SPAM offer", "other@b.org", 0)));
    assert!(!query.matches(&row("hello", "ann@b.org", 0)));
    assert!(!query.matches(&row("hello", "other@b.org", 2)));
}

#[test]
fn negated_flags_flip() {
    assert_eq!(parse("-is:unread").query.unread, Some(false));
    assert_eq!(parse("is:read").query.unread, Some(false));
    assert_eq!(parse("-is:starred").query.starred, Some(false));
}

#[test]
fn a_hostile_pattern_is_refused_not_run() {
    let long = format!("/{}/", "a".repeat(MAX_PATTERN + 1));
    assert_eq!(parse(&long).problem, Some(Problem::LongPattern));
    assert_eq!(parse("/(a{1000}){1000}/").problem, Some(Problem::BadPattern));
    let nested = parse("/(a+)+$/").query;
    let started = std::time::Instant::now();
    assert!(!nested.matches(&row(&format!("{}!", "a".repeat(5000)), "x@y.org", 0)));
    assert!(started.elapsed() < std::time::Duration::from_millis(200));
}

#[test]
fn hints_follow_what_is_typed() {
    assert_eq!(hints("").len(), 9);
    assert!(hints("hello ").is_empty());
    let found: Vec<&str> = hints("f").iter().map(|hint| hint.insert).collect();
    assert_eq!(found, ["from:"]);
    let found: Vec<&str> = hints("word is:").iter().map(|hint| hint.insert).collect();
    assert_eq!(found, ["is:unread", "is:starred"]);
    assert_eq!(hints("-t").first().map(|hint| hint.insert), Some("to:"));
    assert!(hints("xyz").is_empty());
    assert!(hints("from:").is_empty());
}

#[test]
fn a_hint_replaces_the_token_it_finishes() {
    assert_eq!(with_hint("fr", "from:"), "from:");
    assert_eq!(with_hint("hello is:", "is:unread"), "hello is:unread ");
    assert_eq!(with_hint("hello -t", "to:"), "hello -to:");
    assert_eq!(with_hint("", "has:attachment"), "has:attachment ");
    assert_eq!(with_hint("a ", "from:"), "a from:");
}

#[test]
fn the_builder_writes_what_the_parser_reads() {
    let form = Form {
        words: "budget".into(),
        from: "ann".into(),
        to: "bob".into(),
        subject: "big news".into(),
        pattern: r"a/b\d".into(),
        after: "2026-01-01".into(),
        before: "2026-02-01".into(),
        has_attachment: true,
        unread: true,
        starred: true,
    };
    let parsed = parse(&form.text());
    assert_eq!(parsed.problem, None, "{}", form.text());
    let query = parsed.query;
    assert_eq!(query.keyword, ["budget"]);
    assert_eq!(query.from.as_deref(), Some("ann"));
    assert_eq!(query.subject.as_deref(), Some("big news"));
    assert!(query.has_attachment);
    assert_eq!(query.starred, Some(true));
    assert_eq!(query.checks.len(), 1);
    assert_eq!(Form::default().text(), "");
}

#[test]
fn hostile_text_does_not_panic() {
    for input in [
        "\"",
        "/",
        "-",
        "--",
        ":",
        "::",
        "a:",
        "/\\",
        "/\\/",
        "from:\"",
        "from:/",
        "\u{202e}",
        "-\u{44f}:",
        "is:",
        "has:",
    ] {
        let parsed = parse(input);
        assert_eq!(joined(&parsed), input.trim_end_matches(' '), "{input}");
    }
}

#[test]
fn the_builder_opens_on_what_the_field_holds() {
    let form = Form::read(r#"budget from:ann subject:"big news" after:2026-01-01 has:attachment is:starred /a\/b/i"#);
    assert_eq!(form.words, "budget");
    assert_eq!(form.from, "ann");
    assert_eq!(form.subject, "big news");
    assert_eq!(form.after, "2026-01-01");
    assert!(form.has_attachment);
    assert!(form.starred);
    assert!(!form.unread);
    assert_eq!(form.pattern, "a/b");
    assert_eq!(parse(&form.text()).problem, None);
}
