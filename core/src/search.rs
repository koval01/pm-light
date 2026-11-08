//! The search language. What Proton can filter by goes to the server. What it cannot, a regular
//! expression or an exclusion, is checked here against each row of the pages that come back.
//!
//! The text is hostile like any other input: a pattern is bounded in length and in compiled size, and
//! the `regex` crate runs in linear time, so no pattern can hang the app.

use crate::model::MessageMeta;
use chrono::NaiveDate;
use regex::{Regex, RegexBuilder};

const MAX_PATTERN: usize = 256;
const PATTERN_MEMORY: usize = 1 << 18;
const KEYS: [&str; 7] = ["from", "to", "subject", "has", "is", "before", "after"];
const IS_VALUES: [&str; 4] = ["unread", "read", "starred", "unstarred"];
const HAS_VALUE: &str = "attachment";
/// A typing mistake this close to an operator is reported as one, and anything further is a word.
const NEAR_KEY: usize = 2;

/// What a piece of the text is, so that a field can colour it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Space,
    Word,
    Key,
    Value,
    Pattern,
    Negation,
    Wrong,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub part: Part,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    OpenQuote,
    OpenPattern,
    BadPattern,
    LongPattern,
    MissingValue(String),
    BadDate(String),
    UnknownValue(String),
    DidYouMean(String),
    WrongOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Any,
    From,
    To,
    Subject,
}

#[derive(Clone, Debug)]
enum Test {
    Contains(String),
    Pattern(Regex),
    HasFiles,
}

/// One condition checked here rather than on the server.
#[derive(Clone, Debug)]
pub struct Check {
    field: Field,
    test: Test,
    negated: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub keyword: Vec<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub subject: Option<String>,
    pub after: Option<u64>,
    pub before: Option<u64>,
    pub has_attachment: bool,
    pub unread: Option<bool>,
    pub starred: Option<bool>,
    pub checks: Vec<Check>,
}

#[derive(Clone, Debug)]
pub struct Parsed {
    pub spans: Vec<Span>,
    pub query: Query,
    /// The first thing wrong, if any. Such a text is not searched for.
    pub problem: Option<Problem>,
}

/// One suggestion while typing: what goes into the text, and which explanation goes with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    pub insert: &'static str,
    pub explain: i32,
}

/// The fields of the search builder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Form {
    pub words: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub pattern: String,
    pub after: String,
    pub before: String,
    pub has_attachment: bool,
    pub unread: bool,
    pub starred: bool,
}

struct Value {
    raw: String,
    text: String,
    part: Part,
    pattern: Option<Pattern>,
    problem: Option<Problem>,
}

struct Pattern {
    source: String,
    ignore_case: bool,
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, left) in a.iter().enumerate() {
        let mut diagonal = row.first().copied().unwrap_or(0);
        if let Some(first) = row.first_mut() {
            *first = i + 1;
        }
        for (j, right) in b.iter().enumerate() {
            let above = row.get(j + 1).copied().unwrap_or(usize::MAX - 1);
            let beside = row.get(j).copied().unwrap_or(usize::MAX - 1);
            let cost = diagonal + usize::from(left != right);
            diagonal = above;
            if let Some(cell) = row.get_mut(j + 1) {
                *cell = cost.min(above + 1).min(beside + 1);
            }
        }
    }
    row.last().copied().unwrap_or(0)
}

fn near_key(word: &str) -> Option<&'static str> {
    KEYS.iter().copied().find(|key| distance(word, key) <= NEAR_KEY.min(key.len().saturating_sub(1)))
}

fn date_of(text: &str) -> Option<u64> {
    let date = NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(text, "%d.%m.%Y"))
        .ok()?;
    u64::try_from(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp()).ok()
}

fn compiled(pattern: &Pattern) -> Result<Regex, Problem> {
    if pattern.source.chars().count() > MAX_PATTERN {
        return Err(Problem::LongPattern);
    }
    RegexBuilder::new(&pattern.source)
        .case_insensitive(pattern.ignore_case)
        .size_limit(PATTERN_MEMORY)
        .dfa_size_limit(PATTERN_MEMORY)
        .build()
        .map_err(|_| Problem::BadPattern)
}

fn collect(chars: &[char], from: usize, to: usize) -> String {
    chars.iter().skip(from).take(to.saturating_sub(from)).collect()
}

fn quoted(chars: &[char], start: usize) -> (usize, Value) {
    let close = (start + 1..chars.len()).find(|at| chars.get(*at) == Some(&'"'));
    let end = close.map_or(chars.len(), |at| at + 1);
    let raw = collect(chars, start, end);
    let text = collect(chars, start + 1, close.unwrap_or(chars.len()));
    let problem = close.is_none().then_some(Problem::OpenQuote);
    (end, Value { raw, text, part: Part::Value, pattern: None, problem })
}

fn slashed(chars: &[char], start: usize) -> (usize, Value) {
    let mut at = start + 1;
    let mut close = None;
    while let Some(c) = chars.get(at) {
        match c {
            '\\' => at += 1,
            '/' => {
                close = Some(at);
                break;
            }
            _ => {}
        }
        at += 1;
    }
    let Some(close) = close else {
        let raw = collect(chars, start, chars.len());
        return (chars.len(), Value { raw, text: String::new(), part: Part::Wrong, pattern: None, problem: Some(Problem::OpenPattern) });
    };
    let mut end = close + 1;
    while chars.get(end).is_some_and(char::is_ascii_alphabetic) {
        end += 1;
    }
    let flags = collect(chars, close + 1, end);
    let raw = collect(chars, start, end);
    let source = collect(chars, start + 1, close);
    if flags.chars().any(|flag| flag != 'i') {
        return (end, Value { raw, text: String::new(), part: Part::Wrong, pattern: None, problem: Some(Problem::BadPattern) });
    }
    let pattern = Pattern { source, ignore_case: !flags.is_empty() };
    (end, Value { raw, text: String::new(), part: Part::Pattern, pattern: Some(pattern), problem: None })
}

fn value_at(chars: &[char], start: usize) -> (usize, Value) {
    match chars.get(start) {
        Some('"') => quoted(chars, start),
        Some('/') => slashed(chars, start),
        _ => {
            let end = (start..chars.len()).find(|at| chars.get(*at).is_none_or(|c| c.is_whitespace())).unwrap_or(chars.len());
            let text = collect(chars, start, end);
            (end, Value { raw: text.clone(), text, part: Part::Value, pattern: None, problem: None })
        }
    }
}

struct Reader {
    spans: Vec<Span>,
    query: Query,
    problem: Option<Problem>,
}

impl Reader {
    fn push(&mut self, text: String, part: Part) {
        if !text.is_empty() {
            self.spans.push(Span { text, part });
        }
    }

    fn fail(&mut self, problem: Problem) {
        self.problem.get_or_insert(problem);
    }

    /// A condition on a field: to the server when it can take it, or else a check here.
    fn on_field(&mut self, field: Field, value: Value, negated: bool) {
        if let Some(pattern) = &value.pattern {
            match compiled(pattern) {
                Ok(regex) => {
                    self.push(value.raw, Part::Pattern);
                    self.query.checks.push(Check { field, test: Test::Pattern(regex), negated });
                }
                Err(problem) => {
                    self.push(value.raw, Part::Wrong);
                    self.fail(problem);
                }
            }
            return;
        }
        let text = value.text.clone();
        let (raw, part) = (value.raw, value.part);
        let part = if field == Field::Any && part == Part::Value { Part::Word } else { part };
        self.push(raw, part);
        if negated {
            self.query.checks.push(Check { field, test: Test::Contains(text.to_lowercase()), negated });
            return;
        }
        match field {
            Field::Any => self.query.keyword.push(text),
            Field::From => self.query.from = Some(text),
            Field::To => self.query.to = Some(text),
            Field::Subject => self.query.subject = Some(text),
        }
    }

    fn on_date(&mut self, key: &str, value: Value) {
        let found = date_of(&value.text);
        let part = if found.is_some() { Part::Value } else { Part::Wrong };
        self.push(value.raw, part);
        match (found, key) {
            (None, _) => self.fail(Problem::BadDate(value.text)),
            (Some(at), "after") => self.query.after = Some(at),
            (Some(at), _) => self.query.before = Some(at),
        }
    }

    fn on_is(&mut self, value: Value, negated: bool) {
        let known = IS_VALUES.contains(&value.text.as_str());
        self.push(value.raw, if known { Part::Value } else { Part::Wrong });
        match value.text.as_str() {
            "unread" => self.query.unread = Some(!negated),
            "read" => self.query.unread = Some(negated),
            "starred" => self.query.starred = Some(!negated),
            "unstarred" => self.query.starred = Some(negated),
            _ => self.fail(Problem::UnknownValue(value.text)),
        }
    }

    fn on_has(&mut self, value: Value, negated: bool) {
        let known = value.text == HAS_VALUE;
        self.push(value.raw, if known { Part::Value } else { Part::Wrong });
        if !known {
            self.fail(Problem::UnknownValue(value.text));
        } else if negated {
            self.query.checks.push(Check { field: Field::Any, test: Test::HasFiles, negated });
        } else {
            self.query.has_attachment = true;
        }
    }

    fn on_key(&mut self, key: &str, value: Value, negated: bool) {
        if let Some(problem) = value.problem.clone() {
            self.push(value.raw, Part::Wrong);
            self.fail(problem);
            return;
        }
        if value.raw.is_empty() {
            self.fail(Problem::MissingValue(key.to_string()));
            return;
        }
        match key {
            "from" => self.on_field(Field::From, value, negated),
            "to" => self.on_field(Field::To, value, negated),
            "subject" => self.on_field(Field::Subject, value, negated),
            "is" => self.on_is(value, negated),
            "has" => self.on_has(value, negated),
            _ => self.on_date(key, value),
        }
    }
}

fn key_at(chars: &[char], start: usize) -> Option<(String, usize)> {
    let end = (start..chars.len()).find(|at| !chars.get(*at).is_some_and(char::is_ascii_alphabetic))?;
    (end > start && chars.get(end) == Some(&':')).then(|| (collect(chars, start, end), end))
}

#[must_use]
pub fn parse(input: &str) -> Parsed {
    let chars: Vec<char> = input.chars().collect();
    let mut reader = Reader { spans: Vec::new(), query: Query::default(), problem: None };
    let mut at = 0;
    while at < chars.len() {
        let start = at;
        while chars.get(at).is_some_and(|c| c.is_whitespace()) {
            at += 1;
        }
        reader.push(collect(&chars, start, at), Part::Space);
        if at >= chars.len() {
            break;
        }
        let negated = chars.get(at) == Some(&'-') && chars.get(at + 1).is_some_and(|c| !c.is_whitespace());
        if negated {
            reader.push("-".to_string(), Part::Negation);
            at += 1;
        }
        let key = key_at(&chars, at);
        if let Some((name, colon)) = key.as_ref().filter(|(name, _)| KEYS.contains(&name.as_str())) {
            reader.push(collect(&chars, at, colon + 1), Part::Key);
            let (end, value) = value_at(&chars, colon + 1);
            at = end;
            reader.on_key(name, value, negated);
            continue;
        }
        let (end, value) = value_at(&chars, at);
        if let Some(near) = key.as_ref().and_then(|(name, _)| near_key(name)) {
            reader.fail(Problem::DidYouMean(near.to_string()));
            reader.push(value.raw, Part::Wrong);
        } else if let Some(problem) = value.problem.clone() {
            reader.fail(problem);
            reader.push(value.raw, Part::Wrong);
        } else {
            reader.on_field(Field::Any, value, negated);
        }
        at = end;
    }
    if let (Some(after), Some(before)) = (reader.query.after, reader.query.before)
        && after >= before
    {
        reader.fail(Problem::WrongOrder);
    }
    Parsed { spans: reader.spans, query: reader.query, problem: reader.problem }
}

impl Query {
    #[must_use]
    pub fn keyword_text(&self) -> Option<String> {
        (!self.keyword.is_empty()).then(|| self.keyword.join(" "))
    }

    #[must_use]
    pub const fn checks_here(&self) -> bool {
        !self.checks.is_empty()
    }

    #[must_use]
    pub fn matches(&self, row: &MessageMeta) -> bool {
        self.checks.iter().all(|check| check.matches(row))
    }
}

impl Check {
    fn matches(&self, row: &MessageMeta) -> bool {
        let found = match &self.test {
            Test::HasFiles => row.attachment_count > 0,
            test => texts_of(row, self.field).iter().any(|text| test_text(test, text)),
        };
        found != self.negated
    }
}

fn test_text(test: &Test, text: &str) -> bool {
    match test {
        Test::Contains(wanted) => text.to_lowercase().contains(wanted),
        Test::Pattern(regex) => regex.is_match(text),
        Test::HasFiles => false,
    }
}

fn texts_of(row: &MessageMeta, field: Field) -> Vec<String> {
    let address = |a: &crate::model::Address| [a.name.clone().unwrap_or_default(), a.email.clone()];
    let from = || address(&row.from).to_vec();
    let to = || row.to.iter().flat_map(address).collect::<Vec<String>>();
    match field {
        Field::Any => {
            let mut all = vec![row.subject.clone(), row.snippet.clone()];
            all.extend(from());
            all.extend(to());
            all
        }
        Field::From => from(),
        Field::To => to(),
        Field::Subject => vec![row.subject.clone()],
    }
}

fn last_token(input: &str) -> &str {
    input.rsplit(char::is_whitespace).next().unwrap_or("")
}

/// What could come next at the end of the text.
#[must_use]
pub fn hints(input: &str) -> Vec<Hint> {
    const OPERATORS: [Hint; 9] = [
        Hint { insert: "from:", explain: 1 },
        Hint { insert: "to:", explain: 2 },
        Hint { insert: "subject:", explain: 3 },
        Hint { insert: "has:attachment", explain: 4 },
        Hint { insert: "is:unread", explain: 5 },
        Hint { insert: "is:starred", explain: 6 },
        Hint { insert: "after:", explain: 7 },
        Hint { insert: "before:", explain: 8 },
        Hint { insert: "/\\d+/i", explain: 9 },
    ];
    let token = last_token(input);
    // A finished search shows its results, not a menu over them.
    if token.is_empty() && !input.is_empty() {
        return Vec::new();
    }
    let bare = token.strip_prefix('-').unwrap_or(token).to_lowercase();
    OPERATORS.iter().copied().filter(|hint| hint.insert.starts_with(&bare) && hint.insert != bare).collect()
}

/// The text with its last token replaced by a chosen suggestion.
#[must_use]
pub fn with_hint(input: &str, insert: &str) -> String {
    let token = last_token(input);
    let kept = input.strip_suffix(token).unwrap_or(input);
    let negation = if token.starts_with('-') { "-" } else { "" };
    let space = if insert.ends_with(':') { "" } else { " " };
    format!("{kept}{negation}{insert}{space}")
}

fn unquoted(text: &str) -> &str {
    text.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')).unwrap_or(text)
}

fn pattern_source(text: &str) -> Option<String> {
    let inner = text.strip_prefix('/')?.strip_suffix('i').unwrap_or(text.strip_prefix('/')?);
    Some(inner.strip_suffix('/')?.replace("\\/", "/"))
}

impl Form {
    /// The fields for a text, for the builder to open on. What they cannot hold is left out.
    #[must_use]
    pub fn read(input: &str) -> Self {
        let mut form = Self::default();
        let mut key = String::new();
        for span in parse(input).spans {
            match span.part {
                Part::Key => key = span.text.trim_end_matches(':').to_string(),
                Part::Word => {
                    if !form.words.is_empty() {
                        form.words.push(' ');
                    }
                    form.words.push_str(unquoted(&span.text));
                }
                Part::Pattern if key.is_empty() => form.pattern = pattern_source(&span.text).unwrap_or_default(),
                Part::Value => {
                    let value = unquoted(&span.text).to_string();
                    match (key.as_str(), value.as_str()) {
                        ("from", _) => form.from = value,
                        ("to", _) => form.to = value,
                        ("subject", _) => form.subject = value,
                        ("after", _) => form.after = value,
                        ("before", _) => form.before = value,
                        ("has", HAS_VALUE) => form.has_attachment = true,
                        ("is", "unread") => form.unread = true,
                        ("is", "starred") => form.starred = true,
                        _ => {}
                    }
                    key.clear();
                }
                _ => {}
            }
        }
        form
    }
}

fn as_value(text: &str) -> String {
    if text.chars().any(char::is_whitespace) { format!("\"{text}\"") } else { text.to_string() }
}

impl Form {
    /// The search text these fields stand for.
    #[must_use]
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let words = self.words.trim();
        if !words.is_empty() {
            parts.push(words.to_string());
        }
        for (key, value) in [("from", &self.from), ("to", &self.to), ("subject", &self.subject), ("after", &self.after), ("before", &self.before)] {
            let value = value.trim();
            if !value.is_empty() {
                parts.push(format!("{key}:{}", as_value(value)));
            }
        }
        if self.has_attachment {
            parts.push(format!("has:{HAS_VALUE}"));
        }
        if self.unread {
            parts.push("is:unread".to_string());
        }
        if self.starred {
            parts.push("is:starred".to_string());
        }
        let pattern = self.pattern.trim();
        if !pattern.is_empty() {
            parts.push(format!("/{}/i", pattern.replace('/', "\\/")));
        }
        parts.join(" ")
    }
}

/// The dates a search covers, as Proton takes them: `End` is the last second before the day named.
#[must_use]
pub fn begin_and_end(query: &Query) -> (Option<u64>, Option<u64>) {
    (query.after, query.before.map(|at| at.saturating_sub(1)))
}

#[cfg(test)]
mod tests;
