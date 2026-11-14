use super::*;

#[test]
fn a_proof_goes_with_the_code_when_there_is_one() {
    let body = proof_fields("p", "e", "s", Some("123456"), json!({}));
    assert_eq!(body, json!({"ClientProof": "p", "ClientEphemeral": "e", "SRPSession": "s", "TwoFactorCode": "123456"}));
}

#[test]
fn a_proof_without_a_code_says_nothing_of_one() {
    assert!(proof_fields("p", "e", "s", None, json!({})).get("TwoFactorCode").is_none());
}

#[test]
fn what_a_call_adds_cannot_be_lost() {
    assert_eq!(proof_fields("p", "e", "s", None, json!({"Email": "a@b.c"})).get("Email"), Some(&json!("a@b.c")));
}

#[test]
fn an_account_name_cannot_break_out_of_the_uri() {
    let uri = totp_uri("a b?c@d.e", "SECRET");
    assert!(uri.starts_with("otpauth://totp/a"));
    assert_eq!(uri.matches('?').count(), 1);
}
