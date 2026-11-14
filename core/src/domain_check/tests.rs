use super::*;

fn strings(all: &[&str]) -> Vec<String> {
    all.iter().map(|text| (*text).to_string()).collect()
}

#[test]
fn the_verification_code_must_match_exactly() {
    assert!(verified(strings(&["other", " protonmail-verification=abc "]), "protonmail-verification=abc").ok);
    assert!(!verified(strings(&["protonmail-verification=abd"]), "protonmail-verification=abc").ok);
    assert!(!verified(strings(&[""]), "").ok);
}

#[test]
fn proton_must_come_first_with_its_backup() {
    let good = vec![(20, "mailsec.protonmail.ch.".to_string()), (10, "MAIL.protonmail.ch.".to_string())];
    let finding = mail_hosts(good);
    assert!(finding.ok);
    assert_eq!(finding.found[0], "10 mail.protonmail.ch");
    assert!(!mail_hosts(vec![(10, "mail.protonmail.ch.".to_string())]).ok);
    let behind = vec![
        (1, "mx.other.example.".to_string()),
        (10, "mail.protonmail.ch.".to_string()),
        (20, "mailsec.protonmail.ch.".to_string()),
    ];
    assert!(!mail_hosts(behind).ok);
    assert!(!mail_hosts(vec![]).ok);
}

#[test]
fn one_spf_record_that_includes_proton() {
    assert!(spf(&strings(&["v=spf1 include:_spf.protonmail.ch ~all", "unrelated"])).ok);
    assert!(spf(&strings(&["v=spf1 include:_spf.protonmail.ch include:spf.example.com ~all"])).ok);
    assert!(!spf(&strings(&["v=spf1 ~all"])).ok);
    assert!(!spf(&strings(&["v=spf1 include:_spf.protonmail.ch ~all", "v=spf1 ~all"])).ok);
    assert!(!spf(&strings(&["include:_spf.protonmail.ch"])).ok);
}

#[test]
fn one_dmarc_record_with_a_policy() {
    assert!(dmarc(&strings(&["v=DMARC1; p=quarantine"])).ok);
    assert!(!dmarc(&strings(&["v=DMARC1"])).ok);
    assert!(!dmarc(&strings(&[])).ok);
    assert!(!dmarc(&strings(&["v=DMARC1; p=none", "v=DMARC1; p=reject"])).ok);
}

#[test]
fn every_dkim_record_must_point_where_proton_says() {
    let pair = |expected: &str, found: &[&str]| (expected.to_string(), strings(found));
    assert!(dkim(vec![pair("a.proton.ch.", &["A.proton.ch."]), pair("b.proton.ch", &["b.proton.ch."])]).ok);
    assert!(!dkim(vec![pair("a.proton.ch", &["a.proton.ch"]), pair("b.proton.ch", &[])]).ok);
    assert!(!dkim(vec![]).ok);
}
