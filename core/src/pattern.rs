//! Regular expressions written into the code. `Regex::new` returns a `Result` even for a pattern
//! known when the program is built, and turning that into a panic is what the lints forbid: mail is
//! hostile input, and a panic on it is a crash anyone can send. A pattern that does not compile
//! matches nothing here, and `pattern/tests.rs` compiles every one of ours, so none ever is.

use regex::{Captures, Regex, Replacer, bytes};
use std::{borrow::Cow, sync::OnceLock};

pub struct Pattern {
    source: &'static str,
    compiled: OnceLock<Option<Regex>>,
}

impl Pattern {
    pub const fn new(source: &'static str) -> Self {
        Self { source, compiled: OnceLock::new() }
    }

    fn regex(&self) -> Option<&Regex> {
        self.compiled.get_or_init(|| Regex::new(self.source).ok()).as_ref()
    }

    #[cfg(test)]
    pub fn compiles(&self) -> bool {
        self.regex().is_some()
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.regex().is_some_and(|regex| regex.is_match(text))
    }

    pub fn captures<'t>(&self, text: &'t str) -> Option<Captures<'t>> {
        self.regex()?.captures(text)
    }

    pub fn captures_iter<'r, 't>(&'r self, text: &'t str) -> impl Iterator<Item = Captures<'t>> + use<'r, 't> {
        self.regex().into_iter().flat_map(move |regex| regex.captures_iter(text))
    }

    pub fn replace_all<'t>(&self, text: &'t str, replacer: impl Replacer) -> Cow<'t, str> {
        self.regex().map_or(Cow::Borrowed(text), |regex| regex.replace_all(text, replacer))
    }
}

/// The same over bytes, for mail before its charset is known.
pub struct BytesPattern {
    source: &'static str,
    compiled: OnceLock<Option<bytes::Regex>>,
}

impl BytesPattern {
    pub const fn new(source: &'static str) -> Self {
        Self { source, compiled: OnceLock::new() }
    }

    fn regex(&self) -> Option<&bytes::Regex> {
        self.compiled.get_or_init(|| bytes::Regex::new(self.source).ok()).as_ref()
    }

    #[cfg(test)]
    pub fn compiles(&self) -> bool {
        self.regex().is_some()
    }

    pub fn find<'t>(&self, text: &'t [u8]) -> Option<bytes::Match<'t>> {
        self.regex()?.find(text)
    }

    pub fn replace_all<'t>(&self, text: &'t [u8], replacer: impl bytes::Replacer) -> Cow<'t, [u8]> {
        self.regex().map_or(Cow::Borrowed(text), |regex| regex.replace_all(text, replacer))
    }
}

/// A group of a match, or nothing when the group took no part in it: indexing would panic there.
pub fn group<'t>(captures: &Captures<'t>, index: usize) -> &'t str {
    captures.get(index).map_or("", |found| found.as_str())
}

#[cfg(test)]
mod tests;
