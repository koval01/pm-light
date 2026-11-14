use super::*;
use proton_core::print::PrintPart;

#[test]
fn a_document_cannot_end_the_script_it_travels_in() {
    let prepared = PrintDocument {
        style: "a{}".to_string(),
        fit: "b{}".to_string(),
        parts: vec![PrintPart { marks: String::new(), html: "\"});alert(1);//".to_string() }],
    };
    let script = placing(&prepared).unwrap_or_default();
    assert!(script.contains(r#"\"});alert(1);//"#));
}
