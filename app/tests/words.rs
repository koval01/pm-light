//! The words of the interface: `app/ui` holds the English, `app/lang` the rest, and neither may lack
//! what the other has. A new key goes into `say.slint` and both language files.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "a test that cannot read its files must fail")]

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const LANGUAGES: [&str; 2] = ["ru", "de"];
const SIZE_UNITS: usize = 5;

fn app() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files(folder: &Path, end: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(folder).expect("a folder of the app").flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files(&path, end));
        } else if path.extension().is_some_and(|extension| extension == end) {
            found.push(path);
        }
    }
    found
}

/// The texts between quotes that follow each `start`, escapes kept as written.
fn quoted_after(text: &str, start: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(start) {
        rest = rest.get(at + start.len()..).unwrap_or_default();
        let mut end = None;
        let mut escaped = false;
        for (position, c) in rest.char_indices() {
            match c {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => {
                    end = Some(position);
                    break;
                }
                _ => escaped = false,
            }
        }
        let Some(end) = end else { break };
        found.extend(rest.get(..end).map(str::to_string));
    }
    found
}

/// Slint takes the component a text is written in as its context, so the same English can be
/// translated twice, and each is a text of its own.
type Key = (String, String);

/// Context and English to translation, for one language.
fn translations(language: &str) -> HashMap<Key, String> {
    let path = app().join("lang").join(language).join("LC_MESSAGES/proton-mail-light.po");
    let text = fs::read_to_string(path).expect("a language file");
    let mut found = HashMap::new();
    for block in text.split("\n\n") {
        let block = format!("\n{block}");
        let context = quoted_after(&block, "\nmsgctxt \"").into_iter().next().unwrap_or_default();
        let (ids, strings) = (quoted_after(&block, "\nmsgid \""), quoted_after(&block, "\nmsgstr \""));
        if let (Some(id), Some(string)) = (ids.first(), strings.first()) {
            found.insert((context, id.clone()), string.clone());
        }
    }
    found
}

fn placeholders(text: &str) -> Vec<String> {
    let mut found: Vec<String> =
        text.split('{').skip(1).filter_map(|part| part.split_once('}')).map(|(name, _)| name.to_string()).collect();
    found.sort();
    found
}

/// The component or global a line starts, which is the context of every text inside it.
fn declared(line: &str) -> Option<String> {
    let line = line.strip_prefix("export ").unwrap_or(line);
    let rest = line.strip_prefix("component ").or_else(|| line.strip_prefix("global "))?;
    rest.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).next().map(str::to_string)
}

/// The texts of `@tr(...)` on a line, each with its context: the one written as `@tr("context" => "text")`, or
/// else the component's.
fn texts_on(line: &str, component: &str) -> Vec<Key> {
    let mut found = Vec::new();
    for (at, _) in line.match_indices("@tr(\"") {
        let call = line.get(at..).unwrap_or_default();
        let quoted = quoted_after(call, "\"");
        let first = quoted.first().cloned().unwrap_or_default();
        let after_first = call.get(5 + first.len() + 1..).unwrap_or_default();
        if after_first.trim_start().starts_with("=>") {
            let text = quoted_after(after_first, "\"").into_iter().next().unwrap_or_default();
            found.push((first, text));
        } else {
            found.push((component.to_string(), first));
        }
    }
    found
}

fn english() -> Vec<Key> {
    let mut found = Vec::new();
    for path in files(&app().join("ui"), "slint") {
        let mut context = String::new();
        for line in fs::read_to_string(path).expect("an interface file").lines() {
            if let Some(name) = declared(line) {
                context = name;
            }
            found.extend(texts_on(line, &context));
        }
    }
    found
}

/// English texts that two keys of `say.slint` share, with one meaning, so one translation serves both. Any other
/// text two keys share must be told apart with a context of its own, `@tr("key" => "text")`: otherwise both get one
/// translation, and in Russian "Off" said of a phrase came out in the form for a log.
const SAME_MEANING: [&str; 19] = [
    "Default",
    "Recovery email",
    "Keyboard shortcuts",
    "Not set",
    "Open",
    "Subject",
    "Tor",
    "State",
    "You",
    "Nothing found",
    "Delete",
    "Name",
    "Addresses",
    "Invited",
    "{{used}} of {{max}}",
    "Test sheet",
    "Applies to Proton's own apps. This client sends none.",
    "Filters",
    "As in the language",
];

#[test]
fn keys_that_share_english_mean_the_same() {
    let say = fs::read_to_string(app().join("ui/say.slint")).expect("the words");
    let mut keys_of: HashMap<String, Vec<String>> = HashMap::new();
    for line in say.lines() {
        let Some(key) = quoted_after(line, "if key == \"").into_iter().next() else { continue };
        for (context, text) in texts_on(line, "Say") {
            if context == "Say" {
                keys_of.entry(text).or_default().push(key.clone());
            }
        }
    }
    let mut shared: Vec<String> = keys_of
        .iter()
        .filter(|(text, keys)| keys.len() > 1 && !SAME_MEANING.contains(&text.as_str()))
        .map(|(text, keys)| format!("{text:?}: {keys:?}"))
        .collect();
    shared.sort();
    assert!(shared.is_empty(), "give each a context of its own, or list it as meaning the same: {shared:#?}");
}

/// A language file says each text once: two entries for one text leave it to chance which is used.
#[test]
fn every_text_is_translated_once() {
    for language in LANGUAGES {
        let path = app().join("lang").join(language).join("LC_MESSAGES/proton-mail-light.po");
        let text = fs::read_to_string(path).expect("a language file");
        let mut seen = HashSet::new();
        let mut twice = Vec::new();
        for block in text.split("\n\n") {
            let block = format!("\n{block}");
            let context = quoted_after(&block, "\nmsgctxt \"").into_iter().next().unwrap_or_default();
            if let Some(id) = quoted_after(&block, "\nmsgid \"").into_iter().next()
                && !id.is_empty()
                && !seen.insert((context.clone(), id.clone()))
            {
                twice.push((context, id));
            }
        }
        assert!(twice.is_empty(), "{language} has more than one entry for {twice:#?}");
    }
}

#[test]
fn a_context_written_in_the_text_is_read() {
    assert_eq!(
        texts_on(r#"x: @tr("recovery.off" => "Off");"#, "Say"),
        vec![("recovery.off".to_string(), "Off".to_string())]
    );
    assert_eq!(texts_on(r#"x: @tr("Off");"#, "Say"), vec![("Say".to_string(), "Off".to_string())]);
    assert_eq!(texts_on(r#"a: @tr("A"); b: @tr("c" => "B");"#, "App").len(), 2);
}

#[test]
fn every_text_has_its_translations() {
    let english = english();
    assert!(english.len() > 100, "the interface did not load");
    for language in LANGUAGES {
        let known = translations(language);
        let missing: Vec<&Key> = english.iter().filter(|key| !known.contains_key(*key)).collect();
        assert!(missing.is_empty(), "{language} lacks {missing:#?}");
    }
}

#[test]
fn no_translation_is_empty() {
    for language in LANGUAGES {
        let known = translations(language);
        let empty: Vec<&Key> = known
            .iter()
            .filter(|((_, id), text)| !id.is_empty() && text.trim().is_empty())
            .map(|(key, _)| key)
            .collect();
        assert!(empty.is_empty(), "{language} has nothing for {empty:#?}");
    }
}

#[test]
fn a_translation_keeps_what_rust_fills_in() {
    for language in LANGUAGES {
        for ((context, id), text) in translations(language) {
            assert_eq!(placeholders(&id), placeholders(&text), "{language}: {context}: {id}");
        }
    }
}

#[test]
fn every_key_rust_asks_for_exists() {
    let say = fs::read_to_string(app().join("ui/say.slint")).expect("the words");
    let known: HashSet<String> = quoted_after(&say, "if key == \"").into_iter().collect();
    assert!(known.len() > 100, "the words did not load");
    let mut asked = Vec::new();
    for (folder, end) in [("src", "rs"), ("ui", "slint")] {
        for path in files(&app().join(folder), end) {
            let text = fs::read_to_string(&path).expect("a source file");
            for start in [".say(\"", "say(app, \"", "say(&app, \"", ".say_with(\"", "Say.text(\"", "invoke_text(\""] {
                asked.extend(quoted_after(&text, start));
            }
        }
    }
    let missing: Vec<&String> = asked.iter().filter(|key| !known.contains(*key)).collect();
    assert!(missing.is_empty(), "no such keys: {missing:#?}");
}

#[test]
fn every_language_names_its_five_sizes() {
    let say = fs::read_to_string(app().join("ui/say.slint")).expect("the words");
    let at = say.find("\"units.size\"").expect("the key of sizes");
    let units = quoted_after(say.get(at..).unwrap_or_default(), "@tr(\"");
    let english = units.first().expect("the English sizes");
    assert_eq!(english.split(',').count(), SIZE_UNITS);
    for language in LANGUAGES {
        let key = ("Say".to_string(), english.clone());
        assert_eq!(translations(language)[&key].split(',').count(), SIZE_UNITS, "{language}");
    }
}
