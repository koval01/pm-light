use super::*;
use serde_json::json;

#[test]
fn a_free_account_has_no_organization_to_ask_about() {
    assert!(!may_have_organization(&json!({"Subscribed": 0, "Role": 0})));
    assert!(!may_have_organization(&json!({})));
    assert!(!may_have_organization(&Value::Null));
}

#[test]
fn any_paid_product_may_have_an_organization() {
    // `Subscribed` is a set of products: mail is 1, VPN 4, and any of them is a plan.
    for products in [1, 4, 5, 16] {
        assert!(may_have_organization(&json!({"Subscribed": products})), "{products}");
    }
}

#[test]
fn a_locked_admin_keeps_the_organization_and_no_other_lock_gives_one() {
    assert!(may_have_organization(&json!({"Subscribed": 0, "LockedFlags": 4})));
    assert!(may_have_organization(&json!({"Subscribed": 0, "LockedFlags": 4 | 1 | 16})));
    // Storage full, or a member locked by the organization, or a user with a domain: none of them makes a free
    // account have an organization of its own.
    for flags in [1, 2, 3, 8, 16, 1 | 2 | 8 | 16] {
        assert!(!may_have_organization(&json!({"Subscribed": 0, "LockedFlags": flags})), "{flags}");
    }
}

#[test]
fn only_an_admin_may_read_domains() {
    assert!(may_read_domains(&json!({"Role": 2, "Subscribed": 1})));
    assert!(!may_read_domains(&json!({"Role": 0, "Subscribed": 0})));
    assert!(!may_read_domains(&json!({"Role": 1, "Subscribed": 1})));
    assert!(!may_read_domains(&json!({})));
}

#[test]
fn a_strange_user_is_asked_about_nothing() {
    for user in [
        json!({"Role": "2", "Subscribed": "1", "LockedFlags": "4"}),
        json!({"Role": 2.5, "Subscribed": -1, "LockedFlags": -4}),
        json!({"Role": [2], "Subscribed": {"Mail": 1}, "LockedFlags": null}),
        json!([{"Role": 2, "Subscribed": 1}]),
    ] {
        assert!(!may_have_organization(&user) && !may_read_domains(&user), "{user}");
    }
}
