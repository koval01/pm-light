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

/// Proton, for an account described by `user`: it refuses the organization of one that has none, as it does.
fn proton_for(user: &'static str, organization: (u16, &'static str)) -> crate::test_server::Server {
    crate::test_server::serve(move |path, _| {
        Some(match path.split('?').next().unwrap_or_default() {
            "/core/v4/users" => (200, format!(r#"{{"Code":1000,"User":{user}}}"#)),
            "/core/v4/organizations" => (organization.0, organization.1.to_string()),
            "/core/v4/members" => (
                200,
                r#"{"Code":1000,"Members":[{"Name":"A","Role":2,"Self":1,"State":1},{"Name":"B","Role":1,"State":1}]}"#
                    .to_string(),
            ),
            _ => (500, String::new()),
        })
    })
}

const NO_ORGANIZATION: (u16, &str) = (422, r#"{"Code":2011,"Error":"No organization"}"#);
const AN_ORGANIZATION: (u16, &str) =
    (200, r#"{"Code":1000,"Organization":{"Name":"Org","UsedMembers":2,"MaxMembers":5}}"#);

fn asked(server: &crate::test_server::Server, path: &str) -> bool {
    server.seen.lock().iter().any(|seen| seen.starts_with(path))
}

#[test]
fn a_free_account_is_not_asked_about_an_organization() {
    let server = proton_for(r#"{"Subscribed":0,"Role":0}"#, NO_ORGANIZATION);
    let service = super::super::tests::signed_in_at(&server.base);
    let found = super::super::tests::finishes(async move { service.organization().await }).unwrap();
    assert_eq!(found, api::to_organization(&Value::Null, &Value::Null));
    assert!(!asked(&server, "/core/v4/organizations"));
    assert!(!asked(&server, "/core/v4/members"));
}

#[test]
fn a_paying_account_is_asked_about_its_organization_and_its_people() {
    let server = proton_for(r#"{"Subscribed":1,"Role":2}"#, AN_ORGANIZATION);
    let service = super::super::tests::signed_in_at(&server.base);
    let found = super::super::tests::finishes(async move { service.organization().await }).unwrap();
    assert_eq!((found.name.as_str(), found.members, found.people.len()), ("Org", (2, 5), 2));
    assert!(asked(&server, "/core/v4/organizations") && asked(&server, "/core/v4/members"));
}

#[test]
fn an_admin_locked_after_the_plan_lapsed_still_sees_the_organization() {
    let server = proton_for(r#"{"Subscribed":0,"Role":2,"LockedFlags":4}"#, AN_ORGANIZATION);
    let service = super::super::tests::signed_in_at(&server.base);
    let found = super::super::tests::finishes(async move { service.organization().await }).unwrap();
    assert_eq!(found.people.len(), 2);
    assert!(asked(&server, "/core/v4/organizations"));
}

#[test]
fn a_paying_account_refused_all_the_same_has_no_organization() {
    let server = proton_for(r#"{"Subscribed":1,"Role":2}"#, NO_ORGANIZATION);
    let service = super::super::tests::signed_in_at(&server.base);
    let found = super::super::tests::finishes(async move { service.organization().await }).unwrap();
    assert!(found.people.is_empty());
    assert!(!asked(&server, "/core/v4/members"));
}

#[test]
fn without_the_account_there_is_no_organization_page() {
    let server = crate::test_server::serve(|_, _| Some((500, "")));
    let service = super::super::tests::signed_in_at(&server.base);
    assert!(super::super::tests::finishes(async move { service.organization().await }).is_err());
    assert!(!asked(&server, "/core/v4/organizations"));
}
