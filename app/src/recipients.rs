//! Who a message goes to. Addresses are objects here: a separator or a chosen contact turns what was
//! typed into one, and it is removed as a whole. The window only draws what this keeps.

use crate::{Chip, ChipLine, ChipLines, ContactHint};
use proton_core::{
    compose::{domain_of, is_valid_address, suggestions, typed_recipients},
    model::ContactEmail,
};
use slint::{ModelRc, VecModel};
use std::{collections::HashSet, rc::Rc};

pub const TO: usize = 0;
pub const CC: usize = 1;
pub const BCC: usize = 2;
const HINTS: usize = 5;
// Only decides where a line ends, so it runs on the safe side of the widest address.
const CHIP_CHAR: f32 = 7.2;
const CHIP_EDGES: f32 = 38.0;
const CHIP_GAP: f32 = 4.0;
const ROOM_TO_TYPE: f32 = 140.0;

#[derive(Default)]
pub struct People {
    fields: [Vec<String>; 3],
    hints: Vec<ContactEmail>,
    active: usize,
    chosen: usize,
    /// Domains already asked about, and those of them that take no mail.
    checked: HashSet<String>,
    dead: HashSet<String>,
}

impl People {
    pub fn new(to: &str, cc: &str, bcc: &str) -> Self {
        Self { fields: [to, cc, bcc].map(typed_recipients), ..Self::default() }
    }

    pub fn list(&self, field: usize) -> &[String] {
        self.fields.get(field).map_or(&[], Vec::as_slice)
    }

    pub fn joined(&self, field: usize) -> String {
        self.list(field).join(", ")
    }

    pub fn is_empty(&self) -> bool {
        self.fields.iter().all(Vec::is_empty)
    }

    pub fn has_bad(&self) -> bool {
        self.fields.iter().flatten().any(|address| self.is_bad(address))
    }

    fn is_bad(&self, address: &str) -> bool {
        domain_of(address).is_none_or(|domain| self.dead.contains(&domain))
    }

    /// Domains nobody has asked about yet. Asking once is enough for a letter.
    pub fn unchecked_domains(&mut self) -> Vec<String> {
        let mut fresh = Vec::new();
        for domain in self.fields.iter().flatten().filter_map(|address| domain_of(address)) {
            if self.checked.insert(domain.clone()) {
                fresh.push(domain);
            }
        }
        fresh
    }

    pub fn mark_dead(&mut self, domain: String) {
        self.dead.insert(domain);
    }

    pub fn hints(&self) -> &[ContactEmail] {
        &self.hints
    }

    pub const fn chosen(&self) -> usize {
        self.chosen
    }

    fn add(&mut self, field: usize, address: &str) {
        let Some(list) = self.fields.get_mut(field) else { return };
        if !address.is_empty() && !list.iter().any(|known| known.eq_ignore_ascii_case(address)) {
            list.push(address.to_string());
        }
    }

    fn clear_hints(&mut self) {
        self.hints.clear();
        self.chosen = 0;
    }

    /// The field after an edit: every finished address has become an object, and the rest stays
    /// to be typed on. A paste can bring several at once.
    pub fn typed(&mut self, field: usize, text: &str, contacts: &[ContactEmail]) -> String {
        self.active = field;
        let separated = text.ends_with(|c: char| c.is_whitespace() || c == ',' || c == ';');
        let mut parts = typed_recipients(text);
        let rest = if separated { String::new() } else { parts.pop().unwrap_or_default() };
        for part in parts {
            self.add(field, &part);
        }
        self.hints = suggestions(contacts, &rest, self.list(field), HINTS);
        self.chosen = 0;
        rest
    }

    /// A separator was pressed: a valid address is taken as it is, and anything else as the
    /// contact it was the start of, or as a mistake to show.
    pub fn commit(&mut self, field: usize, text: &str) {
        let typed = text.trim();
        let address = match self.hints.get(self.chosen) {
            Some(hint) if !is_valid_address(typed) => hint.email.clone(),
            _ => typed.to_string(),
        };
        self.add(field, &address);
        self.clear_hints();
    }

    /// The field was left: only what was written counts, not a guess.
    pub fn left(&mut self, field: usize, text: &str) {
        self.add(field, text.trim());
    }

    pub fn pick(&mut self, index: usize) {
        if let Some(hint) = self.hints.get(index).cloned() {
            self.add(self.active, &hint.email);
        }
        self.clear_hints();
    }

    pub fn step(&mut self, delta: i32) {
        let last = self.hints.len().saturating_sub(1);
        self.chosen = if delta < 0 { self.chosen.saturating_sub(1) } else { (self.chosen + 1).min(last) };
    }

    pub fn remove(&mut self, field: usize, index: usize) {
        if let Some(list) = self.fields.get_mut(field).filter(|list| index < list.len()) {
            list.remove(index);
        }
    }

    pub fn remove_last(&mut self, field: usize) {
        if let Some(list) = self.fields.get_mut(field) {
            list.pop();
        }
    }
}

fn estimated_width(address: &str) -> f32 {
    crate::numbers::float_of(address.chars().count()).mul_add(CHIP_CHAR, CHIP_EDGES)
}

/// Which addresses stand on each line. The last line also holds the place to type in, so when it
/// has no room left for that, all of it moves up.
fn lines_of(list: &[String], room: f32) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut full = Vec::new();
    let mut line = Vec::new();
    let mut used = 0.0;
    for (index, address) in list.iter().enumerate() {
        let width = estimated_width(address) + CHIP_GAP;
        if used + width > room && !line.is_empty() {
            full.push(std::mem::take(&mut line));
            used = 0.0;
        }
        line.push(index);
        used += width;
    }
    if used + ROOM_TO_TYPE > room && !line.is_empty() {
        full.push(std::mem::take(&mut line));
    }
    (full, line)
}

fn chips(people: &People, list: &[String], indexes: &[usize]) -> ModelRc<Chip> {
    let row: Vec<Chip> = indexes
        .iter()
        .filter_map(|&index| {
            let address = list.get(index)?;
            Some(Chip {
                text: address.as_str().into(),
                index: i32::try_from(index).unwrap_or(0),
                bad: people.is_bad(address),
            })
        })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(row)))
}

pub fn chip_lines(people: &People, field: usize, room: f32) -> ChipLines {
    let list = people.list(field);
    let (full, last) = lines_of(list, room);
    let lines: Vec<ChipLine> = full.iter().map(|line| ChipLine { chips: chips(people, list, line) }).collect();
    ChipLines { lines: ModelRc::from(Rc::new(VecModel::from(lines))), last: chips(people, list, &last) }
}

pub fn hint_rows(hints: &[ContactEmail]) -> ModelRc<ContactHint> {
    let rows: Vec<ContactHint> = hints
        .iter()
        .map(|hint| ContactHint { name: hint.name.as_str().into(), email: hint.email.as_str().into() })
        .collect();
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

#[cfg(test)]
mod tests;
