use super::*;

fn area_for(body: &str, plain: bool) -> String {
    writing_area(&ComposeState { body: body.into(), plain, ..compose::compose_prepared(&[], "", "") }, false)
}

#[test]
fn a_plain_letter_keeps_the_lines_it_starts_with() {
    // One more than the letter has: the parser takes it away again.
    assert!(area_for("\n\n\n\nwords", true).contains(">\n\n\n\n\nwords</textarea>"));
}

#[test]
fn a_plain_letter_cannot_close_its_field() {
    let area = area_for("</textarea><script>x</script>", true);
    assert_eq!(area.matches("</textarea>").count(), 1);
    assert!(!area.contains("<script>"));
}

#[test]
fn a_rich_letter_stays_in_the_editor() {
    assert!(area_for("<b>words</b>", false).contains(r#"contenteditable="true"><b>words</b></div>"#));
}
