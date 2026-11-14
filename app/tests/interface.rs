//! Rules of the window that a mistake breaks without a word.

#![allow(clippy::expect_used, clippy::indexing_slicing, reason = "a test that cannot read its files must fail")]

use std::{fs, path::PathBuf};

fn window() -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/app.slint")).expect("the window")
}

/// An element behind `if` gets its preferred size, not its parent's, and is centred: a screen meant to cover the
/// window came out invisible that way. Every one directly in the window is placed and sized by hand.
fn unplaced(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let start = lines.iter().position(|line| line.starts_with("export component App")).expect("the App component");
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(start) {
        let is_shown_if = line.starts_with("    if ") && line.trim_end().ends_with('{') && line.contains(": ");
        if !is_shown_if {
            continue;
        }
        let body: Vec<&str> = lines.iter().skip(index + 1).take(16).copied().collect();
        let sets = |property: &str| body.iter().any(|line| line.starts_with(&format!("        {property}:")));
        if !(sets("x") && sets("y") && sets("height")) {
            found.push(format!("line {}: {}", index + 1, line.trim()));
        }
    }
    found
}

#[test]
fn every_element_shown_by_a_condition_in_the_window_is_placed() {
    let found = unplaced(&window());
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_check_catches_an_element_left_to_its_own_size() {
    let source =
        "export component App inherits Window {\n    if root.open: Rectangle {\n        background: red;\n    }\n}\n";
    assert_eq!(unplaced(source).len(), 1);
    let placed = "export component App inherits Window {\n    if root.open: Rectangle {\n        x: 0;\n        y: 0;\n        height: 10px;\n    }\n}\n";
    assert!(unplaced(placed).is_empty());
}

/// The question about Tor comes before anything goes out, so it must cover everything, the splash's logo too.
#[test]
fn the_question_about_tor_is_drawn_last() {
    let source = window();
    let question = source.find("if root.asking-tor: Rectangle").expect("the question");
    let logo = source.find("    Logo {").expect("the logo");
    let splash = source.find("splash := Rectangle").expect("the splash");
    assert!(question > logo && question > splash);
}

fn settings() -> String {
    fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui/settings.slint")).expect("the settings")
}

/// Slint gives a scroller the height of its content as if every wrapped text were one line, while it draws the
/// text wrapped: the journal could not be scrolled to its end that way, once its records ran onto more lines. A
/// wrapped text in the settings states its own height, which Slint does measure at its real width.
fn wrapped_without_height(source: &str) -> Vec<usize> {
    let lines: Vec<&str> = source.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() != "wrap: word-wrap;" && line.trim() != "wrap: char-wrap;" {
            continue;
        }
        // The element the line belongs to: from its opening brace to its closing one.
        let start = (0..index).rev().find(|&at| lines[at].trim_end().ends_with('{')).unwrap_or(0);
        let editable =
            lines[start..index].iter().any(|line| line.contains("single-line: false") || line.contains("TextInput"));
        let depth = lines[start].len() - lines[start].trim_start().len();
        let end = (index..lines.len())
            .find(|&at| lines[at].trim() == "}" && lines[at].len() - lines[at].trim_start().len() == depth)
            .unwrap_or(lines.len());
        let sized = lines[start..end].iter().any(|line| line.trim() == "height: self.preferred-height;");
        if !editable && !sized {
            found.push(index + 1);
        }
    }
    found
}

#[test]
fn every_wrapped_text_in_the_settings_states_its_height() {
    let source = settings();
    assert!(source.matches("wrap: word-wrap;").count() >= 7, "the wrapped texts were not found");
    let found = wrapped_without_height(&source);
    assert!(found.is_empty(), "settings.slint lines {found:?}");
}

#[test]
fn the_check_catches_a_wrapped_text_left_to_slint() {
    let bare = "        Text {\n            text: root.line.title;\n            wrap: word-wrap;\n        }\n";
    assert_eq!(wrapped_without_height(bare), vec![3]);
    let sized = "        Text {\n            wrap: word-wrap;\n            height: self.preferred-height;\n        }\n";
    assert!(wrapped_without_height(sized).is_empty());
    let field = "        TextInput {\n            single-line: false;\n            wrap: word-wrap;\n        }\n";
    assert!(wrapped_without_height(field).is_empty());
}

/// The icon buttons of a part of the window, each as written, with the line it starts on.
fn icon_buttons(source: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (at, _) in source.match_indices("IconButton {") {
        let mut depth = 0usize;
        let mut end = source.len();
        for (offset, c) in source.get(at..).unwrap_or_default().char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = at + offset + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        let line = source.get(..at).unwrap_or_default().lines().count() + 1;
        found.push((line, source.get(at..end).unwrap_or_default().to_string()));
    }
    found
}

/// A picture alone does not say what a button does: every icon button of the main window has a hint.
#[test]
fn every_icon_button_of_the_window_says_what_it_does() {
    let source = window();
    let start = source.find("component SideItem").expect("the window's parts");
    let end = source.find("export component InfoWindow").expect("the end of the window");
    let part = source.get(start..end).expect("the window");
    let buttons = icon_buttons(part);
    assert!(buttons.len() > 25, "only {} icon buttons were found", buttons.len());
    let silent: Vec<usize> =
        buttons.iter().filter(|(_, button)| !button.contains("hint:")).map(|(line, _)| *line).collect();
    assert!(silent.is_empty(), "icon buttons without a hint, by line from SideItem: {silent:?}");
}

#[test]
fn the_check_finds_a_button_without_a_hint() {
    let source = "IconButton { icon: x; clicked => { a(); } }\nIconButton {\n    hint: @tr(\"B\");\n    clicked => { b(); }\n}\n";
    let buttons = icon_buttons(source);
    assert_eq!(buttons.len(), 2);
    assert!(!buttons[0].1.contains("hint:") && buttons[1].1.contains("hint:"));
}

/// The inbox count is set in one place, which shows it in the window, the Dock and the tray together: set from two,
/// the badge followed one and stayed behind.
#[test]
fn the_inbox_count_is_set_in_one_place() {
    let view = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/view.rs")).expect("the view");
    assert_eq!(view.matches("set_inbox_unread(").count(), 1, "the inbox count is set in more than one place");
    let at = view.find("fn show_inbox_unread").expect("the one place");
    let body = view.get(at..).and_then(|rest| rest.split("\n}\n").next()).unwrap_or_default();
    assert!(body.contains("set_inbox_unread(") && body.contains("show_unread("), "{body}");
}
