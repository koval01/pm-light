//! Finding things by a few typed letters: commands in the palette, tabs in the settings.

use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Lower case without accents, so a query needs neither.
fn fold(text: &str) -> String {
    text.to_lowercase().nfd().filter(|c| !is_combining_mark(*c)).collect()
}

/// A start beats a word start beats the middle, and letters merely in order come last.
fn score(text: &str, query: &str) -> Option<u8> {
    if let Some(at) = text.find(query) {
        let word_start = text.split_at_checked(at).is_some_and(|(before, _)| before.ends_with(' '));
        return Some(if at == 0 { 3 } else { u8::from(word_start) + 1 });
    }
    let mut letters = text.chars();
    query.chars().all(|wanted| letters.any(|c| c == wanted)).then_some(0)
}

/// The positions of the labels a query matches, best first and otherwise in their own order.
#[must_use]
pub fn rank(labels: &[String], query: &str) -> Vec<usize> {
    let wanted = fold(query.trim());
    if wanted.is_empty() {
        return (0..labels.len()).collect();
    }
    let mut found: Vec<(usize, u8)> =
        labels.iter().enumerate().filter_map(|(index, label)| Some((index, score(&fold(label), &wanted)?))).collect();
    found.sort_by_key(|(_, rank)| std::cmp::Reverse(*rank));
    found.into_iter().map(|(index, _)| index).collect()
}

/// The positions of the texts holding the query whole: settings have long descriptions, in which
/// letters in order would match nearly everything.
#[must_use]
pub fn containing(texts: &[String], query: &str) -> Vec<usize> {
    let wanted = fold(query.trim());
    let holds = |text: &String| fold(text).contains(&wanted);
    texts.iter().enumerate().filter_map(|(index, text)| holds(text).then_some(index)).collect()
}

#[cfg(test)]
mod tests;
