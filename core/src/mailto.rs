//! A `mailto:` link, which any page on the web can hand to the app. All it can do is fill in a letter
//! for a person to read: whatever else it asks for, an attachment above all, is ignored.

use percent_encoding::percent_decode_str;
use serde::Serialize;
use url::Url;

/// Far more than a real link holds, and a limit on what a page can make the composer take in.
const LONGEST: usize = 64 * 1024;

/// A message the link asks to start, as the composer wants it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Mailto {
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

fn addresses_in(list: &str) -> impl Iterator<Item = String> + '_ {
    list.split(',').map(str::trim).filter(|address| !address.is_empty()).map(str::to_string)
}

#[must_use]
pub fn parse(link: &str) -> Option<Mailto> {
    if link.len() > LONGEST {
        return None;
    }
    let url = Url::parse(link).ok()?;
    if url.scheme() != "mailto" {
        return None;
    }
    let path = percent_decode_str(url.path()).decode_utf8_lossy();
    let mut mailto = Mailto { to: addresses_in(&path).collect(), ..Default::default() };
    for (name, value) in url.query_pairs() {
        match name.to_ascii_lowercase().as_str() {
            "to" => mailto.to.extend(addresses_in(&value)),
            "subject" => mailto.subject = value.into_owned(),
            "body" => mailto.body = value.into_owned(),
            _ => {}
        }
    }
    Some(mailto)
}

#[cfg(test)]
mod tests;
