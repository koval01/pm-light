//! What a settings page is made of. Each function gives one kind of row, which `settings.slint` knows
//! how to draw, and the pages only say which ones they hold.

use super::Ctx;
pub use crate::SettingsRow;
use crate::{CodeLine, CodePiece, SettingsSection};
use proton_core::{
    sieve::highlight::{self, TokenKind},
    timing,
};
use slint::{Image, ModelRc, SharedString, VecModel};
use std::rc::Rc;

const CODE_LENGTH: usize = 6;
const ENTRY: i32 = 0;
const SWITCH: i32 = 1;
const SEGMENTED: i32 = 2;
const SWATCHES: i32 = 3;
const NOTE: i32 = 4;
const HINT: i32 = 5;
const FIELD: i32 = 6;
const BUTTON: i32 = 7;
const METER: i32 = 8;
const CHOICE: i32 = 9;
const ERROR: i32 = 10;
const KEYS: i32 = 11;
const RECORD: i32 = 12;
const STRONG: i32 = 13;
const TAGS: i32 = 14;
const FILTER: i32 = 15;
const PICTURE: i32 = 16;
const CODE: i32 = 17;
const HOP: i32 = 18;

fn row(kind: i32) -> SettingsRow {
    SettingsRow { kind, enabled: true, ..SettingsRow::default() }
}

fn texts(options: Vec<String>) -> ModelRc<SharedString> {
    ModelRc::from(Rc::new(VecModel::from(options.into_iter().map(SharedString::from).collect::<Vec<_>>())))
}

pub fn section(id: &str, title: &str, action: &str, rows: Vec<SettingsRow>) -> SettingsSection {
    SettingsSection {
        id: id.into(),
        title: title.into(),
        action: action.into(),
        card: false,
        rows: ModelRc::from(Rc::new(VecModel::from(rows))),
    }
}

/// One of several alike, in a card of its own.
pub fn card(rows: Vec<SettingsRow>) -> SettingsSection {
    SettingsSection { card: true, ..section("", "", "", rows) }
}

/// Nothing but rows, with no heading.
pub fn plain(rows: Vec<SettingsRow>) -> SettingsSection {
    section("", "", "", rows)
}

/// Where a node of a Tor circuit stands on the rail drawn beside it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
#[expect(clippy::struct_excessive_bools, reason = "four independent ends of a line, each drawn or not")]
pub struct Rail {
    pub first: bool,
    pub last: bool,
    /// The way to the node above cannot be seen from here, and is drawn broken.
    pub hidden_above: bool,
    /// The same for the way to the node below.
    pub hidden_below: bool,
}

impl Rail {
    const fn bits(self) -> i32 {
        (self.first as i32)
            | ((self.last as i32) << 1)
            | ((self.hidden_below as i32) << 2)
            | ((self.hidden_above as i32) << 3)
    }
}

/// A node of a Tor circuit: what it is, what it sees, its country and address on the right, and a long address
/// under it. `own` fills the two ends, the person and Proton.
pub fn hop(title: &str, detail: &str, value: &str, address: &str, rail: Rail, own: bool) -> SettingsRow {
    SettingsRow {
        title: title.into(),
        detail: detail.into(),
        value: value.into(),
        text: address.into(),
        selected: rail.bits(),
        on: own,
        ..row(HOP)
    }
}

/// One thing and what it stands at.
pub fn fact(title: &str, value: &str) -> SettingsRow {
    SettingsRow { title: title.into(), value: value.into(), ..row(ENTRY) }
}

/// The same, with the value out of sight until pointed at.
pub fn hidden_fact(title: &str, value: &str) -> SettingsRow {
    SettingsRow { hidden: true, ..fact(title, value) }
}

pub fn entry(id: &str, title: &str, detail: &str) -> SettingsRow {
    SettingsRow { id: id.into(), title: title.into(), detail: detail.into(), ruled: true, ..row(ENTRY) }
}

pub fn switch(id: &str, title: &str, hint: &str, on: bool) -> SettingsRow {
    SettingsRow { id: id.into(), title: title.into(), detail: hint.into(), on, ..row(SWITCH) }
}

pub fn segmented(id: &str, title: &str, options: Vec<String>, selected: usize) -> SettingsRow {
    let selected = i32::try_from(selected).unwrap_or(0);
    SettingsRow { id: id.into(), title: title.into(), options: texts(options), selected, ..row(SEGMENTED) }
}

pub fn swatches(id: &str, title: &str, options: Vec<String>, selected: usize) -> SettingsRow {
    SettingsRow { kind: SWATCHES, ..segmented(id, title, options, selected) }
}

pub fn note(text: &str) -> SettingsRow {
    SettingsRow { detail: text.into(), ..row(NOTE) }
}

pub fn hint(text: &str) -> SettingsRow {
    SettingsRow { detail: text.into(), ..row(HINT) }
}

pub fn error(text: &str) -> SettingsRow {
    SettingsRow { detail: text.into(), ..row(ERROR) }
}

pub fn strong(text: &str) -> SettingsRow {
    SettingsRow { title: text.into(), ..row(STRONG) }
}

pub fn field(id: &str, title: &str, text: &str) -> SettingsRow {
    SettingsRow { id: id.into(), title: title.into(), text: text.into(), lines: 1, ..row(FIELD) }
}

pub fn secret(id: &str, title: &str) -> SettingsRow {
    SettingsRow { secret: true, ..field(id, title, "") }
}

/// A secret field that takes the keyboard when it appears.
pub fn first_secret(id: &str, title: &str) -> SettingsRow {
    SettingsRow { on: true, ..secret(id, title) }
}

pub fn code(id: &str, title: &str, takes_focus: bool) -> SettingsRow {
    SettingsRow { id: id.into(), title: title.into(), on: takes_focus, ..row(CODE) }
}

/// What a code's cells show for what was typed: digits only, as many as fit.
pub fn code_digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).take(CODE_LENGTH).collect()
}

pub const fn is_whole_code(digits: &str) -> bool {
    digits.len() == CODE_LENGTH
}

pub fn digit_cells(digits: &str) -> ModelRc<SharedString> {
    texts(digits.chars().map(String::from).collect())
}

/// A piece never ends in a space that would not be measured, and a line with nothing on it still
/// takes up its height.
pub fn code_lines(source: &str) -> ModelRc<CodeLine> {
    let mut lines: Vec<Vec<CodePiece>> = vec![Vec::new()];
    for token in highlight::tokens(source) {
        let kind = code_kind(token.kind);
        for (position, part) in token.text.split('\n').enumerate() {
            if position > 0 {
                lines.push(Vec::new());
            }
            if let (false, Some(line)) = (part.is_empty(), lines.last_mut()) {
                let text = part.replace(' ', "\u{a0}").replace('\t', "\u{a0}\u{a0}\u{a0}\u{a0}");
                line.push(CodePiece { text: text.into(), kind });
            }
        }
    }
    let rows = lines.into_iter().map(|mut pieces| {
        if pieces.is_empty() {
            pieces.push(CodePiece { text: "\u{a0}".into(), kind: 0 });
        }
        CodeLine { pieces: ModelRc::from(Rc::new(VecModel::from(pieces))) }
    });
    ModelRc::from(Rc::new(VecModel::from(rows.collect::<Vec<_>>())))
}

const fn code_kind(kind: TokenKind) -> i32 {
    match kind {
        TokenKind::Plain => 0,
        TokenKind::Comment => 1,
        TokenKind::String => 2,
        TokenKind::Keyword => 3,
        TokenKind::Tag => 4,
        TokenKind::Number => 5,
    }
}

pub fn area(id: &str, title: &str, text: &str, lines: i32) -> SettingsRow {
    SettingsRow { lines, ..field(id, title, text) }
}

pub fn button(id: &str, text: &str, enabled: bool) -> SettingsRow {
    SettingsRow { id: id.into(), button: text.into(), enabled, ..row(BUTTON) }
}

pub fn ghost(id: &str, text: &str, enabled: bool) -> SettingsRow {
    SettingsRow { ghost: true, ..button(id, text, enabled) }
}

pub fn meter(used: u64, max: u64, text: &str) -> SettingsRow {
    SettingsRow { share: crate::numbers::share(used, max), value: text.into(), ..row(METER) }
}

pub fn choice(id: &str, title: &str, value: &str) -> SettingsRow {
    SettingsRow { id: id.into(), title: title.into(), value: value.into(), ..row(CHOICE) }
}

pub fn keys(title: &str, keys: &str) -> SettingsRow {
    SettingsRow {
        title: title.into(),
        options: texts(keys.split_whitespace().map(str::to_string).collect()),
        ..row(KEYS)
    }
}

/// A line of the journal: 0 is plain, 1 a warning, 2 an error.
pub fn record(time: &str, area: &str, text: &str, level: i32) -> SettingsRow {
    SettingsRow { value: time.into(), tag: area.into(), title: text.into(), selected: level, ..row(RECORD) }
}

/// Turns a row put together by hand into an entry.
pub fn as_entry(row: SettingsRow) -> SettingsRow {
    SettingsRow { kind: ENTRY, ..row }
}

/// Short marks in a row, each good or bad.
pub fn tags(labels: Vec<String>, good: &[bool]) -> SettingsRow {
    let mask = good.iter().enumerate().filter(|(_, good)| !**good).fold(0, |mask, (position, _)| mask | 1 << position);
    SettingsRow { options: texts(labels), selected: mask, ..row(TAGS) }
}

pub fn picture(picture: Image) -> SettingsRow {
    SettingsRow { picture, ..row(PICTURE) }
}

/// A filter: it can be switched, changed and removed where it stands.
pub fn filter(id: &str, name: &str, on: bool) -> SettingsRow {
    SettingsRow { id: id.into(), title: name.into(), on, ..row(FILTER) }
}

pub fn date_time(seconds: i64) -> String {
    timing::moment_text(seconds)
}

/// The account's dates come in milliseconds.
pub fn moment(milliseconds: u64) -> String {
    date_time(i64::try_from(milliseconds / 1000).unwrap_or(0))
}

/// To the millisecond, for the journal.
pub fn precise_moment(milliseconds: u64) -> String {
    proton_core::timing::precise_text(i64::try_from(milliseconds).unwrap_or(0))
}

pub fn day(milliseconds: u64) -> String {
    timing::day_text(i64::try_from(milliseconds / 1000).unwrap_or(0))
}

/// Bytes, in the units of the language.
pub fn size(ctx: &Ctx, bytes: u64) -> String {
    size_in(&ctx.say("units.size"), bytes)
}

pub fn size_in(units: &str, bytes: u64) -> String {
    let units: Vec<&str> = units.split(',').collect();
    let (mut value, mut unit) = (crate::numbers::approximate(bytes), 0);
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    let name = units.get(unit).copied().unwrap_or_default();
    if unit == 0 || value >= 10.0 { format!("{value:.0} {name}") } else { format!("{value:.1} {name}") }
}

pub fn money(cents: i64, currency: &str) -> String {
    money_in(&crate::look::language(), cents, currency)
}

/// As `toLocaleString` writes it in the languages here. Proton names a currency by its code.
fn money_in(language: &str, cents: i64, currency: &str) -> String {
    let symbol = match currency {
        "EUR" => "€",
        "USD" => "$",
        "GBP" => "£",
        other => other,
    };
    let sign = if cents < 0 { "-" } else { "" };
    let (whole, part) = (cents.unsigned_abs() / 100, cents.unsigned_abs() % 100);
    let (group, point) = match language {
        "en" => (",", "."),
        "de" => (".", ","),
        _ => ("\u{a0}", ","),
    };
    let digits = whole.to_string();
    let lead = digits.len() % 3;
    let mut grouped = String::new();
    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && (position + 3 - lead) % 3 == 0 {
            grouped.push_str(group);
        }
        grouped.push(digit);
    }
    let number = format!("{grouped}{point}{part:02}");
    match language {
        "en" if symbol == currency => format!("{sign}{symbol}\u{a0}{number}"),
        "en" => format!("{sign}{symbol}{number}"),
        _ => format!("{sign}{number}\u{a0}{symbol}"),
    }
}

#[cfg(test)]
mod tests;
