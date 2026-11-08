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
    let mut found: Vec<(usize, u8)> = labels
        .iter()
        .enumerate()
        .filter_map(|(index, label)| Some((index, score(&fold(label), &wanted)?)))
        .collect();
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
mod tests {
    use super::*;

    fn labels(all: &[&str]) -> Vec<String> {
        all.iter().map(|label| (*label).to_string()).collect()
    }

    #[test]
    fn an_empty_query_keeps_everything_in_order() {
        assert_eq!(rank(&labels(&["b", "a"]), "  "), [0, 1]);
    }

    #[test]
    fn a_start_beats_a_word_start_beats_the_middle() {
        let all = labels(&["reinbox", "go inbox", "inbox"]);
        assert_eq!(rank(&all, "inbox"), [2, 1, 0]);
    }

    #[test]
    fn letters_in_order_still_match_last() {
        assert_eq!(rank(&labels(&["moved", "archive"]), "mvd"), [0]);
        assert_eq!(rank(&labels(&["zebra"]), "bz"), Vec::<usize>::new());
    }

    #[test]
    fn neither_case_nor_accents_matter() {
        assert_eq!(rank(&labels(&["\u{dc}ber uns"]), "uber"), [0]);
        assert_eq!(rank(&labels(&["\u{415}\u{449}\u{451}"]), "\u{415}\u{429}\u{415}"), [0]);
    }

    #[test]
    fn multi_byte_text_cannot_break_the_search() {
        assert_eq!(rank(&labels(&["🦀 inbox", "\u{44f}\u{449}\u{438}\u{43a}"]), "inbox"), [0]);
        assert_eq!(rank(&labels(&["\u{44f}\u{449}\u{438}\u{43a} \u{432}\u{445}"]), "\u{432}\u{445}"), [0]);
    }

    #[test]
    fn containing_wants_the_query_whole() {
        let all = labels(&["Undo send", "Under the hood"]);
        assert_eq!(containing(&all, "undo"), [0]);
        assert_eq!(containing(&all, ""), [0, 1]);
    }
}
