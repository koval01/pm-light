use super::{BytesPattern, Pattern};
use std::{fs, path::Path};

fn patterns() -> Vec<&'static Pattern> {
    vec![
        &crate::compose::BRACKETED,
        &crate::compose::VALID_ADDRESS,
        &crate::details::AUTH_RESULT,
        &crate::html::PIXEL_STYLE,
        &crate::html::TAG,
        &crate::mime::ENCODED_WORD,
        &crate::mime::EXTENDED,
        &crate::sieve::highlight::TOKEN,
        &crate::text::ENTITY,
        &crate::text::SPACES,
        &crate::text::TOKEN,
    ]
}

fn byte_patterns() -> Vec<&'static BytesPattern> {
    vec![&crate::mime::BLANK_LINE, &crate::mime::FOLDED]
}

fn written(folder: &Path, needle: &str) -> usize {
    let mut found = 0;
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found += written(&path, needle);
        } else if path.extension().is_some_and(|end| end == "rs")
            && !path.ends_with("pattern.rs")
            && !path.ends_with("pattern/tests.rs")
        {
            found += fs::read_to_string(&path).unwrap_or_default().matches(needle).count();
        }
    }
    found
}

#[test]
fn every_pattern_compiles() {
    assert!(patterns().iter().all(|pattern| pattern.compiles()));
    assert!(byte_patterns().iter().all(|pattern| pattern.compiles()));
}

#[test]
fn every_pattern_in_the_code_is_listed_here() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    assert_eq!(written(&source, " Pattern::new("), patterns().len(), "list a new Pattern above");
    assert_eq!(written(&source, " BytesPattern::new("), byte_patterns().len(), "list a new BytesPattern above");
}

#[test]
fn a_broken_pattern_matches_nothing_and_changes_nothing() {
    static BROKEN: Pattern = Pattern::new("(");
    assert!(!BROKEN.compiles());
    assert!(!BROKEN.is_match("("));
    assert_eq!(BROKEN.replace_all("(a)", "b"), "(a)");
    assert_eq!(BROKEN.captures_iter("(a)").count(), 0);
}
