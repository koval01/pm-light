use super::*;

/// The declarations of the class `hints` put on the element whose text is `text`.
fn declarations_for(hinted: &Hinted, text: &str) -> String {
    let before = hinted.html.split(text).next().unwrap_or_default();
    let class = before.rsplit("class=\"").next().unwrap_or_default().split('"').next().unwrap_or_default();
    let ours = class.split(' ').find(|name| name.starts_with(CLASS)).unwrap_or_default();
    let rule = hinted.style.split(&format!(":where(.{ours}){{")).nth(1).unwrap_or_default();
    rule.split('}').next().unwrap_or_default().to_string()
}

#[test]
fn a_font_tag_becomes_its_colour_family_and_size() {
    let hinted = hints(r##"<font color="#d0021b" face="Verdana, Arial" size="2">x</font>"##);
    assert_eq!(declarations_for(&hinted, ">x<"), r#"color:#d0021b;font-family:"Verdana","Arial";font-size:small;"#);
}

#[test]
fn font_sizes_follow_the_standard() {
    for (size, css) in [
        ("1", "x-small"),
        ("3", "medium"),
        ("7", "xxx-large"),
        ("9", "xxx-large"),
        ("0", "x-small"),
        ("+1", "large"),
        ("-2", "x-small"),
        ("+9", "xxx-large"),
    ] {
        assert_eq!(font_size(size), Some(css), "{size}");
    }
    assert_eq!(font_size("big"), None);
}

#[test]
fn colours_are_names_or_hex_and_nothing_else() {
    assert_eq!(colour("FFFFFF").as_deref(), Some("#ffffff"));
    assert_eq!(colour("#ABC").as_deref(), Some("#abc"));
    assert_eq!(colour("White").as_deref(), Some("white"));
    for hostile in ["red;background:url(x)", "#fff}body{display:none", "", "rgb(1,2,3)", "#12345"] {
        assert_eq!(colour(hostile), None, "{hostile}");
    }
}

#[test]
fn a_family_cannot_end_its_string_or_rule() {
    let hinted = hints(r#"<font face="x&quot;;}body{display:none}">y</font>"#);
    assert!(!hinted.style.contains("display:none}") && !hinted.style.contains("x\";"), "{}", hinted.style);
    assert_eq!(hinted.style.matches('{').count(), hinted.style.matches('}').count());
}

#[test]
fn a_bordered_table_borders_its_cells_and_pads_them() {
    let hinted = hints(r#"<table border="1" cellpadding="6" cellspacing="2"><tr><td>a</td></tr></table>"#);
    assert!(hinted.style.contains("border:1px outset gray;border-spacing:2px;"), "{}", hinted.style);
    assert_eq!(declarations_for(&hinted, ">a<"), "padding:6px;border:1px inset gray;");
}

#[test]
fn a_border_of_zero_draws_none() {
    let hinted = hints(r#"<table border="0"><tr><td>a</td></tr></table>"#);
    assert!(!hinted.style.contains("border"), "{}", hinted.style);
}

#[test]
fn an_inner_table_gives_its_padding_only_to_its_own_cells() {
    let hinted = hints(
        r#"<table cellpadding="9"><tr><td><table cellpadding="2"><tr><td>inner</td></tr></table></td><td>outer</td></tr></table>"#,
    );
    assert_eq!(declarations_for(&hinted, ">inner<"), "padding:2px;");
    assert_eq!(declarations_for(&hinted, ">outer<"), "padding:9px;");
}

#[test]
fn cells_alike_share_one_rule() {
    let cells = "<td>x</td>".repeat(1000);
    let hinted = hints(&format!(r#"<table cellpadding="4"><tr>{cells}</tr></table>"#));
    assert_eq!(hinted.style.matches(":where(").count(), 1, "{}", hinted.style);
}

#[test]
fn a_picture_floats_where_align_says() {
    let hinted = hints(r#"<img src="data:image/png;base64,AA" align="LEFT" alt="l"><img align="right" alt="r">"#);
    assert!(hinted.style.contains("float:left;") && hinted.style.contains("float:right;"), "{}", hinted.style);
}

#[test]
fn a_mail_class_is_kept_beside_ours() {
    let hinted = hints(r#"<font class="own" color="red">x</font>"#);
    assert!(hinted.html.contains(&format!(r#"class="own {CLASS}0""#)), "{}", hinted.html);
}

#[test]
fn cells_take_valign_and_named_colours() {
    let hinted =
        hints(r#"<table><tr valign="top" bgcolor="silver"><td valign="BOTTOM" bgcolor="white">a</td></tr></table>"#);
    assert_eq!(declarations_for(&hinted, ">a<"), "vertical-align:bottom;background-color:white;");
    assert!(hinted.style.contains("vertical-align:top;background-color:silver;"), "{}", hinted.style);
}

#[test]
fn a_mail_without_legacy_attributes_is_left_alone() {
    let html = r#"<p style="color:red">plain <b>mail</b></p><table><tr><td>x</td></tr></table>"#;
    let hinted = hints(html);
    assert!(hinted.style.is_empty());
    assert_eq!(hinted.html, html);
}

#[test]
fn lengths_are_bounded() {
    let hinted = hints(r#"<table cellpadding="99999999999"><tr><td>a</td></tr></table><hr width="250%" size="5">"#);
    assert_eq!(declarations_for(&hinted, ">a<"), "padding:10000px;");
    assert!(hinted.style.contains("height:3px;border-width:0;width:100%;"), "{}", hinted.style);
}

#[test]
fn a_border_with_no_number_draws_one_pixel() {
    let hinted = hints(r"<table border><tr><td>a</td></tr></table>");
    assert!(hinted.style.contains("border:1px outset gray;"), "{}", hinted.style);
    assert_eq!(declarations_for(&hinted, ">a<"), "border:1px inset gray;");
}
