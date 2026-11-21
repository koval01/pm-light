//! The attributes browsers turn into style, written as CSS for an engine that does not read them.
//!
//! Blitz reads `width`, `height`, `bgcolor` in hex and `align` as text alignment, and nothing else of the kind: a
//! newsletter's `<font color>`, table `border` and `cellpadding` and a floated picture were lost (docs/BLITZ.md). The
//! HTML standard's "Rendering" section says what each means. Browsers rank them below every rule of the mail's own,
//! so they become `:where()` rules, of no specificity, put before the mail's CSS. Elements with the same declarations
//! share one class, so a long mail adds few rules.

use super::attributes::non_negative;
use lol_html::{EndTagHandler, RewriteStrSettings, element, html_content::Element, rewrite_str};
use std::{cell::RefCell, collections::HashMap, fmt::Write as _, rc::Rc};

/// The classes this pass adds: a prefix no mail is likely to use, and harmless if one does.
const CLASS: &str = "pmlh";
/// A length past this is not a layout but an attack.
const LONGEST: u64 = 10_000;

/// A mail with its legacy attributes as rules.
pub struct Hinted {
    /// `:where()` rules, for a `<style>` before the mail's own.
    pub style: String,
    /// The mail, with a class on each element a rule is for.
    pub html: String,
}

/// A colour as an attribute gives it: a name, or a hex number with or without its `#`.
fn colour(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 32 {
        return None;
    }
    let hex = value.strip_prefix('#').unwrap_or(value);
    if matches!(hex.len(), 3 | 6) && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(format!("#{}", hex.to_ascii_lowercase()));
    }
    value.bytes().all(|b| b.is_ascii_alphabetic()).then(|| value.to_ascii_lowercase())
}

fn pixels(value: &str) -> Option<u64> {
    non_negative(value).map(|n| n.min(LONGEST))
}

/// A width as an attribute gives it: pixels, or a percentage.
fn length(value: &str) -> Option<String> {
    let n = pixels(value)?;
    Some(if value.trim().ends_with('%') { format!("{}%", n.min(100)) } else { format!("{n}px") })
}

/// `face`: a list of family names, each quoted, with nothing that could end the string or the rule.
fn families(value: &str) -> Option<String> {
    let names: Vec<String> = value
        .split(',')
        .map(|name| name.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect::<String>())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .take(8)
        .map(|name| format!("\"{name}\""))
        .collect();
    (!names.is_empty()).then(|| names.join(","))
}

/// `<font size>`: one to seven, or a step from three.
fn font_size(value: &str) -> Option<&'static str> {
    let value = value.trim();
    let step = |rest: &str| rest.trim().parse::<i64>().ok();
    let size = if let Some(rest) = value.strip_prefix('+') {
        step(rest)?.saturating_add(3)
    } else if let Some(rest) = value.strip_prefix('-') {
        3_i64.saturating_sub(step(rest)?)
    } else {
        step(value)?
    };
    Some(match size.clamp(1, 7) {
        1 => "x-small",
        2 => "small",
        3 => "medium",
        4 => "large",
        5 => "x-large",
        6 => "xx-large",
        _ => "xxx-large",
    })
}

fn word(element: &Element<'_, '_>, name: &str) -> Option<String> {
    element.get_attribute(name).map(|value| value.trim().to_ascii_lowercase())
}

/// What an open table gives its cells.
#[derive(Clone, Copy, Default)]
struct Table {
    padding: Option<u64>,
    border: bool,
}

#[derive(Default)]
struct State {
    style: String,
    classes: HashMap<String, usize>,
    tables: Vec<Table>,
}

impl State {
    /// Puts `declarations` on `element` through a class shared by every element with the same.
    fn mark(&mut self, element: &mut Element<'_, '_>, declarations: &str) -> lol_html::HandlerResult {
        if declarations.is_empty() {
            return Ok(());
        }
        let next = self.classes.len();
        let number = *self.classes.entry(declarations.to_string()).or_insert(next);
        if number == next {
            let _ = write!(self.style, ":where(.{CLASS}{number}){{{declarations}}}");
        }
        let class = match element.get_attribute("class") {
            Some(existing) if !existing.trim().is_empty() => format!("{existing} {CLASS}{number}"),
            _ => format!("{CLASS}{number}"),
        };
        element.set_attribute("class", &class)?;
        Ok(())
    }
}

fn font(element: &Element<'_, '_>) -> String {
    let mut out = String::new();
    if let Some(value) = element.get_attribute("color").as_deref().and_then(colour) {
        let _ = write!(out, "color:{value};");
    }
    if let Some(value) = element.get_attribute("face").as_deref().and_then(families) {
        let _ = write!(out, "font-family:{value};");
    }
    if let Some(value) = element.get_attribute("size").as_deref().and_then(font_size) {
        let _ = write!(out, "font-size:{value};");
    }
    out
}

fn table(element: &Element<'_, '_>) -> (String, Table) {
    let mut out = String::new();
    // `border` with no number in it still draws one pixel.
    let border = element.get_attribute("border").map_or(0, |value| pixels(&value).unwrap_or(1));
    if border > 0 {
        let _ = write!(out, "border:{border}px outset gray;");
    }
    if let Some(spacing) = element.get_attribute("cellspacing").as_deref().and_then(pixels) {
        let _ = write!(out, "border-spacing:{spacing}px;");
    }
    match word(element, "align").as_deref() {
        Some("left") => out.push_str("float:left;"),
        Some("right") => out.push_str("float:right;"),
        Some("center") => out.push_str("margin-left:auto;margin-right:auto;"),
        _ => {}
    }
    if let Some(value) = element.get_attribute("bgcolor").as_deref().and_then(colour) {
        let _ = write!(out, "background-color:{value};");
    }
    let padding = element.get_attribute("cellpadding").as_deref().and_then(pixels);
    (out, Table { padding, border: border > 0 })
}

fn cell(element: &Element<'_, '_>, table: Table) -> String {
    let mut out = String::new();
    if let Some(padding) = table.padding {
        let _ = write!(out, "padding:{padding}px;");
    }
    if table.border {
        out.push_str("border:1px inset gray;");
    }
    if let Some(value) =
        word(element, "valign").filter(|v| matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline"))
    {
        let _ = write!(out, "vertical-align:{value};");
    }
    if let Some(value) = element.get_attribute("bgcolor").as_deref().and_then(colour) {
        let _ = write!(out, "background-color:{value};");
    }
    out
}

fn row(element: &Element<'_, '_>) -> String {
    let mut out = String::new();
    if let Some(value) =
        word(element, "valign").filter(|v| matches!(v.as_str(), "top" | "middle" | "bottom" | "baseline"))
    {
        let _ = write!(out, "vertical-align:{value};");
    }
    if let Some(value) = element.get_attribute("bgcolor").as_deref().and_then(colour) {
        let _ = write!(out, "background-color:{value};");
    }
    out
}

fn picture(element: &Element<'_, '_>) -> String {
    match word(element, "align").as_deref() {
        Some("left") => "float:left;".to_string(),
        Some("right") => "float:right;".to_string(),
        Some("middle" | "center" | "absmiddle") => "vertical-align:middle;".to_string(),
        Some("top") => "vertical-align:top;".to_string(),
        Some("bottom" | "baseline") => "vertical-align:baseline;".to_string(),
        _ => String::new(),
    }
}

fn rule(element: &Element<'_, '_>) -> String {
    let mut out = String::new();
    if let Some(value) = element.get_attribute("color").as_deref().and_then(colour) {
        let _ = write!(out, "border-color:{value};background-color:{value};");
    }
    if let Some(size) = element.get_attribute("size").as_deref().and_then(pixels) {
        let _ = write!(out, "height:{}px;border-width:0;", size.saturating_sub(2).max(1));
    }
    if let Some(width) = element.get_attribute("width").as_deref().and_then(length) {
        let _ = write!(out, "width:{width};");
    }
    out
}

/// The mail with its legacy attributes as rules. A mail the rewriter cannot read stays as it was, with no rules.
#[must_use]
pub fn hints(html: &str) -> Hinted {
    let state = Rc::new(RefCell::new(State::default()));
    let (fonts, tables, cells, rows, pictures, rules) =
        (state.clone(), state.clone(), state.clone(), state.clone(), state.clone(), state.clone());
    let rewritten = rewrite_str(
        html,
        RewriteStrSettings {
            element_content_handlers: vec![
                element!("font", move |el| {
                    let declarations = font(el);
                    fonts.borrow_mut().mark(el, &declarations)
                }),
                element!("table", move |el| {
                    let (declarations, opened) = table(el);
                    tables.borrow_mut().tables.push(opened);
                    let closing = tables.clone();
                    if let Some(handlers) = el.end_tag_handlers() {
                        let close: EndTagHandler<'static> = Box::new(move |_| {
                            closing.borrow_mut().tables.pop();
                            Ok(())
                        });
                        handlers.push(close);
                    }
                    tables.borrow_mut().mark(el, &declarations)
                }),
                element!("td, th", move |el| {
                    let around = cells.borrow().tables.last().copied().unwrap_or_default();
                    let declarations = cell(el, around);
                    cells.borrow_mut().mark(el, &declarations)
                }),
                element!("tr", move |el| {
                    let declarations = row(el);
                    rows.borrow_mut().mark(el, &declarations)
                }),
                element!("img", move |el| {
                    let declarations = picture(el);
                    pictures.borrow_mut().mark(el, &declarations)
                }),
                element!("hr", move |el| {
                    let declarations = rule(el);
                    rules.borrow_mut().mark(el, &declarations)
                }),
            ],
            ..RewriteStrSettings::new()
        },
    );
    rewritten.map_or_else(
        |_| Hinted { style: String::new(), html: html.to_string() },
        |html| Hinted { style: std::mem::take(&mut state.borrow_mut().style), html },
    )
}

#[cfg(test)]
mod tests;
