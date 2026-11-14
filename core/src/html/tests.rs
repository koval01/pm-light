use super::*;

#[test]
fn removes_scripts_and_their_content() {
    let html = protect("<p>a</p><script>alert(1)</script>").html;
    assert!(!html.contains("script") && !html.contains("alert"));
}

#[test]
fn removes_event_handlers() {
    assert!(!protect(r#"<img src="x" onerror="alert(1)"><div onclick="x()">a</div>"#).html.contains("on"));
}

#[test]
fn refuses_javascript_links() {
    assert!(!protect(r#"<a href="javascript:alert(1)">x</a>"#).html.contains("javascript"));
    assert!(!protect(r#"<a href="JaVaScRiPt:alert(1)">x</a>"#).html.to_lowercase().contains("javascript"));
}

#[test]
fn refuses_a_data_link_but_keeps_a_data_image() {
    assert!(!protect(r#"<a href="data:text/html,<script>1</script>">x</a>"#).html.contains("data:"));
    assert!(protect(r#"<img src="data:image/png;base64,AA">"#).html.contains("data:image/png"));
}

#[test]
fn stops_navigation_and_fake_forms() {
    let html = protect(
        r#"<meta http-equiv="refresh" content="0;url=https://evil"><base href="https://evil"><form action="https://evil"><input name="password"></form>"#,
    )
    .html;
    for word in ["meta", "base", "form", "input", "evil"] {
        assert!(!html.contains(word), "{word} left in {html}");
    }
}

#[test]
fn removes_frames_and_plugins() {
    let html = protect(r#"<iframe src="https://a"></iframe><object data="x"></object><embed src="y">"#).html;
    assert!(!html.contains("iframe") && !html.contains("object") && !html.contains("embed"));
}

#[test]
fn keeps_the_formatting_mail_is_made_of() {
    let html =
        protect(r#"<table width="600" cellpadding="4"><tr><td bgcolor="red" style="color:blue">x</td></tr></table>"#)
            .html;
    for word in ["<table", "width=\"600\"", "bgcolor", "color:blue"] {
        assert!(html.contains(word), "{word} missing from {html}");
    }
}

#[test]
fn keeps_style_blocks_and_cleans_them() {
    let html = protect("<style>p{color:red} @import url(x); a{position:fixed}</style><p>x</p>").html;
    assert!(html.contains("<style>") && html.contains("color:red"));
    assert!(!html.contains("@import") && !html.contains("fixed"));
}

#[test]
fn cleans_inline_styles() {
    let html = protect(r#"<div style="position: fixed; background: url(javascript:x)">a</div>"#).html;
    assert!(html.contains("position: static") && !html.contains("javascript"));
}

/// A removed comment must not glue two harmless pieces into a tag that ends the style block.
#[test]
fn a_style_block_cannot_be_left_through_a_comment() {
    for payload in [
        "<style></*x*//style><img src=x onerror=alert(1)></style>",
        "<style><!/**/--></*x*/style></style>",
        "<style>a{b:c}</*x*//style><iframe src=https://evil></iframe></style>",
    ] {
        let html = protect(payload).html;
        assert!(!html.contains("<img") && !html.contains("<iframe"), "escaped: {html}");
        assert_eq!(html.matches("</style").count(), 1, "{html}");
    }
}

#[test]
fn no_angle_bracket_survives_in_a_style_block() {
    let html = protect(r#"<style>a{content:"\3c" }b{background:url(data:image/png,<x)}</style>"#).html;
    let inside = html.split("<style>").nth(1).and_then(|rest| rest.split("</style").next()).unwrap();
    assert!(!inside.contains('<'), "{inside}");
}

#[test]
fn every_way_of_writing_a_remote_image_is_held_back() {
    for src in [
        "https://evil.example/a.png",
        "HTTP://EVIL.EXAMPLE/a.png",
        "  https://evil.example/a.png",
        "ht&#9;tps://evil.example/a.png",
        "ht&#10;tps://evil.example/a.png",
        "https:evil.example/a.png",
        "//evil.example/a.png",
    ] {
        let html = protect(&format!(r#"<img src="{src}">"#));
        assert!(!html.html.contains(" src="), "{src} kept: {}", html.html);
        assert_eq!(html.protection.images.len(), 1, "{src} is not in the report");
    }
}

#[test]
fn an_image_that_is_neither_inline_nor_remote_is_dropped() {
    for src in ["/favicon.svg", "data:text/html,x", "ftp://evil.example/a.png", "/\\evil.example/a.png"] {
        let html = protect(&format!(r#"<img src="{src}">"#)).html;
        assert!(!html.contains(" src="), "{src} kept: {html}");
    }
}

#[test]
fn images_carried_by_the_mail_stay() {
    assert!(protect(r#"<img src="cid:logo@x">"#).html.contains("cid:logo@x"));
    assert!(protect(r#"<img src="DATA:image/png;base64,AA">"#).html.to_lowercase().contains("data:image/png"));
}

#[test]
fn css_strings_cannot_name_a_remote_image_without_slashes() {
    let css = clean_css(r#"a{background:image-set("https:evil.example/a.png" 1x)}"#);
    assert!(!css.contains("evil"), "{css}");
}

#[test]
fn nothing_of_a_mail_reaches_the_app_s_own_document_but_its_content() {
    let html =
        for_editor(r#"<style>body{display:none}</style><p id="x" name="y" class="toast" style="color:red">a</p>"#);
    assert_eq!(html, r#"<p style="color:red">a</p>"#);
}

#[test]
fn the_editor_keeps_the_app_s_own_blocks() {
    let html = for_editor(r#"<div class="protonmail_signature_block">a</div><div class="protonmail_quote x">b</div>"#);
    assert!(html.contains(r#"class="protonmail_signature_block""#), "{html}");
    assert!(!html.contains("protonmail_quote"), "a class list is not one of ours: {html}");
}

#[test]
fn links_open_outside_with_no_referrer() {
    let html = protect(r#"<a href="https://x.io/">x</a>"#).html;
    assert!(html.contains(r#"target="_blank""#) && html.contains("noopener"));
}

#[test]
fn strips_tracking_parameters() {
    assert_eq!(clean_link("https://x.io/a?utm_source=m&utm_medium=e"), "https://x.io/a");
    assert_eq!(clean_link("https://x.io/a?id=7&fbclid=abc"), "https://x.io/a?id=7");
    assert_eq!(clean_link("https://x.io/?UTM_Source=m"), "https://x.io/");
    assert_eq!(clean_link("https://x.io/a?gclid=1#top"), "https://x.io/a#top");
}

#[test]
fn leaves_clean_and_foreign_links_alone() {
    for link in ["https://x.io/a?q=hello%20world&b=%D1%8F", "mailto:a@b.c?utm_source=x", "https://", "x"] {
        assert_eq!(clean_link(link), link);
    }
    assert_eq!(clean_link("https://x.io/?utmost=1"), "https://x.io/?utmost=1");
}

#[test]
fn reports_a_cleaned_link_with_its_original() {
    let p = protect(r#"<a href="https://x.io/?utm_source=a">x</a>"#);
    assert!(p.html.contains(r#"href="https://x.io/""#));
    assert_eq!(p.protection.links[0].from, "https://x.io/?utm_source=a");
}

#[test]
fn lists_remote_images_once_and_makes_them_absolute() {
    let p = protect(r#"<img src="https://a.io/1.png"><img src="https://a.io/1.png"><img src="//b.io/2">"#);
    let urls: Vec<_> = p.protection.images.iter().map(|i| i.url.as_str()).collect();
    assert_eq!(urls, ["https://a.io/1.png", "https://b.io/2"]);
}

#[test]
fn spots_a_tracking_pixel() {
    for html in [
        r#"<img src="https://a.io/p" width="1" height="1">"#,
        r#"<img src="https://a.io/p" height="0">"#,
        r#"<img src="https://a.io/p" style="width:1px">"#,
        r#"<img src="https://a.io/p" style="display:none">"#,
    ] {
        assert!(protect(html).protection.images[0].pixel, "{html}");
    }
    assert!(!protect(r#"<img src="https://a.io/p" width="200">"#).protection.images[0].pixel);
}

#[test]
fn a_data_image_is_no_remote_image() {
    assert!(protect(r#"<img src="data:image/png;base64,AA">"#).protection.images.is_empty());
}

#[test]
fn names_removed_elements_with_their_count() {
    let p = protect(r#"<form><input><input></form><iframe src="x"></iframe><script>1</script>"#);
    assert_eq!(p.protection.removed, ["form", "iframe", "input × 2", "script"]);
}

#[test]
fn does_not_report_what_every_newsletter_has() {
    assert!(protect(r#"<meta charset="utf-8"><title>x</title><p>y</p>"#).protection.removed.is_empty());
}

#[test]
fn title_text_does_not_leak_into_the_body() {
    assert!(!protect("<title>Secret title</title><p>y</p>").html.contains("Secret"));
}

#[test]
fn a_cleaned_mail_has_no_remote_address_left_to_fetch() {
    let html = protect(r#"<img src="https://a.io/1.png" width="9">"#).html;
    assert!(!html.contains(" src="), "{html}");
    assert!(html.contains(r#"data-held-src="https://a.io/1.png""#));
}

#[test]
fn css_cannot_load_a_tracking_image() {
    for css in ["background:url(https://a.io/p.gif)", "background: url( 'http://a.io/p' )", "background:url(//a.io/p)"]
    {
        assert!(!clean_css(css).contains("a.io"), "{css}");
    }
    assert!(clean_css("background:url(data:image/png;base64,AA)").contains("data:image/png"));
}

#[test]
fn escaping_does_not_hide_css_from_the_cleaner() {
    for css in [
        r"background:\75 rl(https://a.io/p)",
        r"background:u\72 l(https://a.io/p)",
        r"@\69mport 'https://a.io/x.css';",
        r"position:\66ixed",
        r"\70osition:fixed",
    ] {
        let clean = clean_css(css);
        assert!(!clean.contains("a.io") && !clean.contains("ixed"), "{css} became {clean}");
    }
}

#[test]
fn fonts_and_scriptish_css_are_gone() {
    for css in [
        "@font-face{font-family:x;src:url(https://a.io/f)}p{color:red}",
        "p{width:expression(alert(1))}",
        "p{behavior:url(x.htc)}",
        "p{-moz-binding:url(x.xml)}",
        "p{background:url(javascript:alert(1))}",
    ] {
        let clean = clean_css(css);
        for word in ["a.io", "expression", "behavior", "binding:url", "javascript"] {
            assert!(!clean.contains(word), "{word} left in {clean}");
        }
    }
}

#[test]
fn ordinary_css_survives() {
    let clean = clean_css("p{color:red;margin:0 auto}@media (max-width:600px){td{display:block!important}}");
    assert!(clean.contains("color:red") && clean.contains("@media") && clean.contains("display:block"));
}

#[test]
fn broken_css_cannot_escape_or_panic() {
    for css in ["}{", "url(", "\"unterminated", "@import", "/* open", "a{b{c{d{", "\\"] {
        let _ = clean_css(css);
    }
    assert!(!protect("<style>a{color:red}</style><style>}</style><script>x</script>").html.contains("<script"));
}

#[test]
fn image_maps_cannot_navigate_the_window() {
    let html =
        protect(r##"<map name="m"><area shape="rect" href="https://evil.example"></map><img usemap="#m">"##).html;
    assert!(!html.contains("evil") && !html.contains("area") && !html.contains("usemap"));
}

#[test]
fn a_mail_nested_past_any_real_layout_becomes_plain_text() {
    let protected = protect(&format!("{}x", "<div>".repeat(5_000)));
    assert_eq!(protected.protection.removed, ["oversized"]);
}

#[test]
fn sloppy_but_ordinary_html_stays_html() {
    let html = "<table><tr><td><p>a<br><img src=x></table>".repeat(5_000);
    assert!(protect(&html).protection.removed.is_empty());
}

#[test]
fn an_oversized_mail_becomes_plain_text() {
    let huge = format!("<script>x</script>{}", "<p>abc</p>".repeat(MAX_TAGS));
    let protected = protect(&huge);
    assert!(!protected.html.contains("<script"));
    assert_eq!(protected.protection.removed, ["oversized"]);
}

#[test]
fn a_style_block_is_cleaned_of_remote_urls_too() {
    let html = protect("<style>body{background:url(https://a.io/p)}</style><p>x</p>").html;
    assert!(!html.contains("a.io"));
}

#[test]
fn a_fetched_image_takes_its_place_and_others_stay_held() {
    let images = HashMap::from([("https://a.io/1".to_string(), "data:image/png;base64,AA".to_string())]);
    let held = protect(r#"<img src="https://a.io/1"><img src="https://a.io/2">"#).html;
    let html = with_images(&held, &images);
    assert!(html.contains(r#"src="data:image/png"#) || html.contains("data:image/png"));
    assert!(html.contains(r#"data-held-src="https://a.io/2""#));
    assert!(!html.contains(r#"data-held-src="https://a.io/1""#));
}

/// Run only in a container without network: see "Attacks" in docs/DEVELOPMENT.md.
#[test]
#[ignore = "CPU-heavy on purpose"]
fn hostile_markup_ends_quickly() {
    use std::time::{Duration, Instant};
    let cases: Vec<(&str, String)> = vec![
        ("nested divs", "<div>".repeat(55_000)),
        ("just under the depth limit", "<div>".repeat(510)),
        ("formatting", "<b></b>".repeat(25_000)),
        ("misnested formatting", "<b><p>x</b></p>".repeat(14_000)),
        ("tables", "<table><tr><td>x</table>".repeat(14_000)),
        ("flat tags past the limit", "<p>".repeat(500_000)),
        ("megabytes of text", "x".repeat(3_900_000)),
        ("huge attribute", format!("<a title=\"{}\">x</a>", "y".repeat(3_900_000))),
        ("many links", "<a href=\"https://x.io/?utm_source=1\">x</a>".repeat(50_000)),
        ("many images", "<img src=\"https://x.io/1\">".repeat(50_000)),
        ("big style", format!("<style>{}</style>", "a{color:red}".repeat(300_000))),
        ("deep css", format!("<style>{}</style>", "a{".repeat(1_000_000))),
        ("junk", "\0\u{fffd}<\0>".repeat(50_000)),
        ("unclosed comments", "<!--".repeat(50_000)),
    ];
    for (name, html) in cases {
        let started = Instant::now();
        let out = protect(&html);
        let took = started.elapsed();
        println!("{name:<26} {:>6} KB -> {:>6} KB in {took:?}", html.len() / 1024, out.html.len() / 1024);
        assert!(took < Duration::from_secs(5), "{name} took {took:?}");
    }
}

#[test]
fn embedded_pictures_come_off_the_page() {
    let html = r#"<p>a</p><img src="data:image/png;base64,AAAA" width="10"><img src="cid:one">"#;
    let held = without_embedded(html).unwrap();
    assert!(!held.contains("data:image") && !held.contains("cid:"));
    assert!(held.contains(r#"width="10""#) && held.contains("<p>a</p>"));
}

#[test]
fn a_held_remote_picture_counts_as_no_embedded_one() {
    let html = protect(r#"<img src="https://example.com/a.png">"#).html;
    assert_eq!(without_embedded(&html), None);
}

#[test]
fn a_mail_with_no_pictures_has_nothing_to_hold_back() {
    assert_eq!(without_embedded("<p>words</p>"), None);
}

#[test]
fn every_way_of_writing_an_embedded_picture_is_held_back() {
    for src in ["DATA:image/png;base64,AA", " data:image/gif;base64,AA", "da\tta:image/png;base64,AA", "CID:one"] {
        let html = format!(r#"<img src="{src}">"#);
        assert!(without_embedded(&html).is_some_and(|held| !held.contains("src=")), "{src}");
    }
}
