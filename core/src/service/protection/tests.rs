use super::super::tests::{finishes, signed_in_at};
use crate::test_server::{Server, serve};

/// Proton, for an account described by `user`: it refuses the domains of anyone but an admin, as it does.
fn proton_for(user: &'static str) -> Server {
    serve(move |path, _| {
        let admin = user.contains(r#""Role":2"#);
        Some(match path.split('?').next().unwrap_or_default() {
            "/core/v4/users" => (200, format!(r#"{{"Code":1000,"User":{user}}}"#)),
            "/core/v4/domains" if admin => {
                (200, r#"{"Code":1000,"Domains":[{"DomainName":"own.example","State":1}]}"#.to_string())
            }
            "/core/v4/domains" => (403, r#"{"Code":2026,"Error":"Permission denied"}"#.to_string()),
            _ => (500, String::new()),
        })
    })
}

fn asked_for_domains(server: &Server) -> bool {
    server.seen.lock().iter().any(|seen| seen.starts_with("/core/v4/domains"))
}

#[test]
fn a_free_account_is_not_asked_for_domains() {
    let server = proton_for(r#"{"Subscribed":0,"Role":0}"#);
    let service = signed_in_at(&server.base);
    assert!(finishes(async move { service.domains().await }).unwrap().is_empty());
    assert!(!asked_for_domains(&server));
}

#[test]
fn a_member_of_an_organization_is_not_asked_for_its_domains() {
    let server = proton_for(r#"{"Subscribed":1,"Role":1}"#);
    let service = signed_in_at(&server.base);
    assert!(finishes(async move { service.domains().await }).unwrap().is_empty());
    assert!(!asked_for_domains(&server));
}

#[test]
fn an_admin_is_asked_for_domains() {
    let server = proton_for(r#"{"Subscribed":1,"Role":2}"#);
    let service = signed_in_at(&server.base);
    let found = finishes(async move { service.domains().await }).unwrap();
    assert_eq!(found.iter().map(|domain| domain.name.as_str()).collect::<Vec<_>>(), ["own.example"]);
    assert!(asked_for_domains(&server));
}

#[test]
fn without_the_account_the_domains_are_not_guessed() {
    let server = serve(|_, _| Some((500, "")));
    let service = signed_in_at(&server.base);
    assert!(finishes(async move { service.domains().await }).is_err());
    assert!(!asked_for_domains(&server));
}
