//! The journal holds what the app did, never who or what about. Every record in both crates is read
//! here, and one that could carry an address, a subject, a name, a search, a secret or the text of an
//! error fails the build. An error goes in by its `kind()`, a path through `clean_path`.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "a test that cannot read its files must fail")]

use std::{
    fs,
    path::{Path, PathBuf},
};

const MACROS: [&str; 5] = ["log::error!(", "log::warn!(", "log::info!(", "log::debug!(", "log::trace!("];
/// Words that name the person's things. A record may not mention them in what it fills in.
const PRIVATE: [&str; 21] = [
    "email",
    "address",
    "addresses",
    "subject",
    "body",
    "html",
    "password",
    "secret",
    "token",
    "query",
    "keyword",
    "url",
    "recipient",
    "sender",
    "contact",
    "file_name",
    "message.meta",
    "to",
    "cc",
    "bcc",
    "from",
];
/// What may stand next to an error: its kind, or a summary made for the journal.
const SAFE_ERROR: [&str; 4] = ["kind(", "failure(", "clean_path(", "error_code"];

fn files(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder).expect("a folder of the project").flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files(&path));
        } else if path.extension().is_some_and(|end| end == "rs") {
            found.push(path);
        }
    }
    found
}

/// Each record's text, from its macro to the bracket that closes it.
fn records(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for start in MACROS {
        for (at, _) in text.match_indices(start) {
            let rest = text.get(at + start.len()..).unwrap_or_default();
            let mut depth = 1;
            let mut end = rest.len();
            let mut in_string = false;
            let mut escaped = false;
            for (position, c) in rest.char_indices() {
                match c {
                    '\\' if in_string && !escaped => escaped = true,
                    '"' if !escaped => in_string = !in_string,
                    '(' if !in_string => depth += 1,
                    ')' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            end = position;
                            break;
                        }
                    }
                    _ => escaped = false,
                }
                if c != '\\' {
                    escaped = false;
                }
            }
            found.push(rest.get(..end).unwrap_or_default().to_string());
        }
    }
    found
}

/// What a record fills in: the names inside `{...}` and every argument after the text.
fn filled_in(record: &str) -> Vec<String> {
    let mut parts = Vec::new();
    for piece in record.split('{').skip(1) {
        if let Some((inside, _)) = piece.split_once('}') {
            let name = inside.split(':').next().unwrap_or_default().trim();
            if !name.is_empty() {
                parts.push(name.to_string());
            }
        }
    }
    if let Some(close) = record.rfind('"') {
        parts.extend(record.get(close + 1..).unwrap_or_default().split(',').map(|arg| arg.trim().to_string()));
    }
    parts.retain(|part| !part.is_empty());
    parts
}

fn problems(record: &str) -> Vec<String> {
    let mut found = Vec::new();
    for part in filled_in(record) {
        let lower = part.to_lowercase();
        let words: Vec<&str> = lower.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).collect();
        if let Some(word) = PRIVATE
            .iter()
            .find(|private| words.iter().any(|word| word == *private || word.ends_with(&format!(".{private}"))))
        {
            found.push(format!("`{part}` is the person's: {word}"));
        }
        let mentions_error = words.iter().any(|word| *word == "error" || *word == "e" || word.starts_with("error."));
        if mentions_error && !SAFE_ERROR.iter().any(|safe| lower.contains(safe)) {
            found.push(format!("`{part}` is an error's own text: use its kind()"));
        }
        let names: Vec<&str> = lower.split(|c: char| !(c.is_alphanumeric() || c == '_')).collect();
        if !lower.contains("clean_path(") && names.iter().any(|name| *name == "path" || *name == "name") {
            found.push(format!("`{part}` is a path or a name: use clean_path, or leave it out"));
        }
    }
    found
}

#[test]
fn no_record_carries_what_belongs_to_the_person() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the project");
    let mut checked = 0;
    let mut failed = Vec::new();
    for folder in ["core/src", "app/src"] {
        for path in files(&root.join(folder)) {
            if path.to_string_lossy().contains("tests") {
                continue;
            }
            let text = fs::read_to_string(&path).expect("a source file");
            for record in records(&text) {
                checked += 1;
                failed.extend(problems(&record).into_iter().map(|why| format!("{}: {why}", path.display())));
            }
        }
    }
    assert!(checked > 80, "only {checked} records were found");
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn the_check_itself_catches_what_it_is_for() {
    for record in [
        r#""sending to {}", mail.to"#,
        r#""opened {subject}""#,
        r#""failed: {error}""#,
        r#""failed: {}", error"#,
        r#""failed: {e}""#,
        r#""saved {}", path.display()"#,
        r#""searching {}", keyword"#,
        r#""from {}", message.meta.from.email"#,
    ] {
        assert!(!problems(record).is_empty(), "{record} must be caught");
    }
    for record in [
        r#""failed: {}", error.kind()"#,
        r#""no webview: {}", crate::webviews::failure(&error)"#,
        r#""{method} {} -> {status}", clean_path(path)"#,
        r#""a message opened in {} ms: {} files", started.elapsed().as_millis(), shown.attachments.len()"#,
    ] {
        assert!(problems(record).is_empty(), "{record} is fine: {:?}", problems(record));
    }
}
