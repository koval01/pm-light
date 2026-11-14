use super::*;

#[test]
fn drops_script_and_style() {
    let html =
        "<html><head><style>.a{color:red}</style></head><body><script>alert(1)</script><p>Only this.</p></body></html>";
    assert_eq!(to_text(html), "Only this.");
}

#[test]
fn keeps_paragraph_and_line_breaks() {
    assert_eq!(to_text("<p>First.</p><p>Second.</p>"), "First.\nSecond.");
    assert_eq!(to_text("a<br>b"), "a\nb");
}

#[test]
fn list_items_get_bullets() {
    assert_eq!(to_text("<ul><li>one<li>two</ul>"), "- one\n- two");
}

#[test]
fn unescapes_entities() {
    assert_eq!(to_text("<p>3 &lt; 5 &amp; caf&#233; &#x1F600;</p>"), "3 < 5 & café 😀");
}

#[test]
fn leaves_an_unknown_entity_alone() {
    assert_eq!(to_text("a &nosuch; b &#0; c"), "a &nosuch; b &#0; c");
}

#[test]
fn ignores_comments() {
    assert_eq!(to_text("a<!-- <p>no</p> -->b"), "ab");
}

#[test]
fn a_void_tag_does_not_open_a_skip() {
    assert_eq!(to_text("<meta charset=x><p>ok</p>"), "ok");
}

#[test]
fn malformed_html_still_yields_text() {
    let out = to_text("<div><p>text <b>bold</div></p></b>");
    assert!(out.contains("text") && out.contains("bold") && !out.contains('<'));
}

#[test]
fn keeps_a_stray_less_than() {
    assert!(to_text("<p>1 < 2</p>").contains("1 < 2"));
}

#[test]
fn shrinks_a_marketing_mail() {
    let html = "<table><tr><td>&nbsp;</td></tr></table>".repeat(200) + "<p>Your statement is ready.</p>";
    let text = to_text(&html);
    assert!(text.contains("Your statement is ready."));
    assert!(text.len() < html.len() / 10);
}

#[test]
fn collapse_leaves_at_most_one_blank_line() {
    assert_eq!(collapse("a\n\n\n\n\nb"), "a\n\nb");
}

#[test]
fn plain_text_cannot_inject_markup() {
    let out = plaintext_to_html("a < b & c > d <script>x</script>");
    assert!(out.contains("&lt;") && out.contains("&amp;") && out.contains("&gt;"));
    assert!(!out.contains("<script>"));
}

#[test]
fn newlines_become_breaks_and_paragraphs() {
    assert!(plaintext_to_html("one\ntwo").contains("one<br>"));
    assert_eq!(plaintext_to_html("a\n\nb").matches("<p>").count(), 2);
}

#[test]
fn plain_text_round_trips() {
    let back = to_text(&plaintext_to_html("Hello.\nThis is a < test > with & symbols.\n\nSecond paragraph."));
    assert!(back.contains("a < test > with & symbols.") && back.contains("Second paragraph."));
}

#[test]
fn an_empty_body_is_still_valid_html() {
    assert_eq!(plaintext_to_html(""), "<p></p>");
}
