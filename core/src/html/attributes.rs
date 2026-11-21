//! Attributes an engine reads as numbers or colours, kept to what the HTML standard lets them mean.
//!
//! Browsers clamp a table's spans themselves; Blitz did not, and a `colspan` of 40,000 overflowed its column counter
//! (docs/BLITZ.md).

use lol_html::html_content::Element;

/// The most columns a cell may span, as browsers have it.
pub const MOST_COLUMNS: u64 = 1000;
/// The most rows a cell may span, as browsers have it.
pub const MOST_ROWS: u64 = 65_534;

/// HTML's rules for parsing a non-negative integer.
///
/// Spaces, an optional plus, then digits, and whatever follows is ignored. Digits beyond any count read as the largest
/// number. Nothing when there are no digits.
#[must_use]
pub fn non_negative(value: &str) -> Option<u64> {
    let rest = value.trim_start_matches([' ', '\t', '\n', '\r', '\u{c}']);
    let rest = rest.strip_prefix('+').unwrap_or(rest);
    let count = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (digits, _) = rest.split_at_checked(count)?;
    if digits.is_empty() {
        return None;
    }
    Some(digits.parse::<u64>().unwrap_or(u64::MAX))
}

/// What a span attribute becomes: nothing (the default of one) when it is no number, or zero where zero means one,
/// and at most `most`. `None` drops the attribute.
#[must_use]
pub fn span(value: &str, most: u64, zero_allowed: bool) -> Option<u64> {
    let number = non_negative(value)?;
    if number == 0 && !zero_allowed {
        return None;
    }
    Some(number.min(most))
}

fn limit(element: &mut Element<'_, '_>, name: &str, most: u64, zero_allowed: bool) {
    let Some(value) = element.get_attribute(name) else { return };
    match span(&value, most, zero_allowed) {
        Some(number) => {
            if element.set_attribute(name, &number.to_string()).is_err() {
                element.remove_attribute(name);
            }
        }
        None => element.remove_attribute(name),
    }
}

/// `colspan` and `rowspan` as a browser would read them.
pub fn limit_spans(element: &mut Element<'_, '_>) {
    limit(element, "colspan", MOST_COLUMNS, false);
    // A row span of zero reaches to the end of its group.
    limit(element, "rowspan", MOST_ROWS, true);
}

/// A colour attribute holds a name or a hex number, all ASCII. Anything else is dropped: Blitz reads `bgcolor` by
/// byte offsets, and a letter of two bytes made it panic.
pub fn keep_plain_colours(element: &mut Element<'_, '_>) {
    for name in ["bgcolor", "color"] {
        if element.get_attribute(name).is_some_and(|value| !value.is_ascii()) {
            element.remove_attribute(name);
        }
    }
}

#[cfg(test)]
mod tests;
