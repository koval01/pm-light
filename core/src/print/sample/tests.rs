use super::*;

#[test]
fn the_sample_is_one_mail_with_its_heading_and_body() {
    let labels = PrintLabels { from: "From".into(), date_format: "%d.%m.%Y %H:%M".into(), ..PrintLabels::default() };
    let made = sample_document(&PrintOptions::default(), &labels);
    assert_eq!(made.parts.len(), 1);
    let html = made.parts.first().map(|part| part.html.as_str()).unwrap_or_default();
    assert!(
        html.contains("A sample mail") && html.contains("Sample Sender") && html.contains("The end of the sample.")
    );
    assert!(html.contains(SQUARE), "its picture travels inside it");
}

#[test]
fn the_sample_needs_nothing_from_outside_but_its_one_link() {
    let body = body();
    assert!(!body.contains("src=\"http"));
    assert_eq!(body.matches("href=").count(), 1);
}

#[test]
fn the_square_is_an_svg_of_the_logo_blue() {
    use base64::Engine as _;
    let coded = SQUARE.strip_prefix("data:image/svg+xml;base64,").unwrap_or_default();
    let svg = base64::engine::general_purpose::STANDARD.decode(coded).unwrap_or_default();
    let svg = String::from_utf8(svg).unwrap_or_default();
    assert!(svg.starts_with("<svg") && svg.contains("#2f62dc"), "{svg}");
}
