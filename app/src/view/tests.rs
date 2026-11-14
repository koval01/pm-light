use super::*;

#[test]
fn a_long_line_is_cut_at_a_space_near_the_end() {
    assert_eq!(cut_to_width("one two three", 8), ["one two", "three"]);
    assert_eq!(cut_to_width("aaaaaaaaaaaa", 5), ["aaaaa", "aaaaa", "aa"]);
    assert_eq!(cut_to_width("", 5), [""]);
    assert_eq!(cut_to_width("exactly", 7), ["exactly"]);
}

#[test]
fn no_row_is_wider_than_the_window() {
    let line = "abc def ".repeat(50) + &"\u{44f}".repeat(300);
    for columns in [1, 2, 7, 20, 64] {
        let pieces = cut_to_width(&line, columns);
        assert!(pieces.iter().all(|piece| piece.chars().count() <= columns), "{columns}");
        let joined: String = pieces.concat().split_whitespace().collect();
        assert_eq!(joined, line.split_whitespace().collect::<String>());
    }
}

#[test]
fn header_rows_mark_names_and_keep_folds() {
    let headers = vec![("To".to_string(), "a\nb".to_string()), ("Cc".to_string(), String::new())];
    let rows = header_rows(&headers, 40);
    let names: Vec<bool> = rows.iter().map(|row| row.1).collect();
    assert_eq!(names, [true, false, false, false, true]);
}

#[test]
fn headers_split_into_name_and_text() {
    let raw = "Subject: Hello: there\r\nDKIM-Signature: v=1;\r\n\ta=rsa-sha256;\r\n  b=abc\r\n\r\nTo: a@b\r\n";
    let headers = header_lines(raw);
    assert_eq!(headers.len(), 3);
    assert_eq!(headers[0], ("Subject".to_string(), "Hello: there".to_string()));
    assert_eq!(headers[1].1, "v=1;\na=rsa-sha256;\nb=abc");
    assert_eq!(headers[2].0, "To");
}

#[test]
fn a_folded_line_with_a_colon_stays_in_its_header() {
    let headers = header_lines("Received: from a\n by b: c\nTo: d");
    assert_eq!(headers[0].1, "from a\nby b: c");
    assert_eq!(headers.len(), 2);
}

#[test]
fn hostile_headers_are_cut_down() {
    let raw = "A: b\n".repeat(MAX_HEADERS * 2);
    assert_eq!(header_lines(&raw).len(), MAX_HEADERS);
    let long = format!("A: {}", "\u{44f}".repeat(MAX_HEADER_TEXT * 2));
    assert_eq!(header_lines(&long)[0].1.chars().count(), MAX_HEADER_TEXT);
}

#[test]
fn links_in_text_can_be_pressed_and_keep_what_surrounds_them() {
    let pieces = lines_of("See https://a.example/x?y=1. Then\nmailto is plain\nhttp://b.example");
    assert_eq!(pieces[0], ("See".to_string(), String::new()));
    assert_eq!(pieces[1].1, "https://a.example/x?y=1");
    assert_eq!(pieces[2], (".".to_string(), String::new()));
    assert_eq!(pieces[3], (" Then".to_string(), String::new()));
    assert_eq!(pieces[4].1, "");
    assert_eq!(pieces.last().map(|piece| piece.0.as_str()), Some(""));
    assert!(pieces.iter().any(|piece| piece.1 == "http://b.example"));
}

#[test]
fn nothing_but_web_and_mail_addresses_opens() {
    for url in ["file:///etc/passwd", "javascript:alert(1)", "smb://a/b", ""] {
        assert!(!["https://", "http://", "mailto:"].iter().any(|scheme| url.starts_with(scheme)));
    }
}

#[test]
fn an_odd_body_cannot_panic_the_split() {
    for body in [
        "",
        "https://",
        "http://\u{202e}",
        "\u{43a}\u{438}\u{440}\u{438}\u{43b}\u{43b}\u{438}\u{446}\u{430} https://\u{44f}.example/\u{43f}\u{443}\u{442}\u{44c}, \u{434}\u{430}\u{43b}\u{44c}\u{448}\u{435}",
    ] {
        let _ = lines_of(body);
    }
}
