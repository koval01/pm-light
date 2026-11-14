use super::*;

fn refusal(status: u16) -> Result<Value> {
    Err(Error::Api { status, code: 2501, message: String::new() })
}

#[test]
fn a_plan_without_an_organization_is_not_a_failure() {
    assert_eq!(without_a_plan_for_it(refusal(422)).ok(), Some(Value::Null));
    assert_eq!(without_a_plan_for_it(refusal(403)).ok(), Some(Value::Null));
}

#[test]
fn a_real_failure_stays_one() {
    assert!(without_a_plan_for_it(refusal(500)).is_err());
}
