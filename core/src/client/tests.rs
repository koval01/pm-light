use super::*;
use crate::error::Link;
use crate::test_server::{script, serve};
use std::{sync::Arc, time::Duration};

const REFUSED: &str = r#"{"Code":10013,"Error":"Invalid refresh token"}"#;
const NEW_TOKENS: &str = r#"{"Code":1000,"AccessToken":"new-access","RefreshToken":"new-refresh","UID":"uid"}"#;
const FINE: &str = r#"{"Code":1000}"#;

/// A client holding a session, and a record of what it reported about it.
fn holding_a_session(base: &str) -> (Arc<Client>, Arc<Mutex<Vec<String>>>) {
    let client = Arc::new(Client::new(base).unwrap());
    client.set_session(Some(Session {
        uid: "uid".into(),
        access_token: "access".into(),
        refresh_token: "refresh".into(),
        password_mode: 1,
    }));
    let changes = Arc::new(Mutex::new(Vec::new()));
    let record = changes.clone();
    client.on_session_change(Box::new(move |change| {
        record.lock().push(match change {
            SessionChange::Renewed(session) => format!("renewed {}", session.access_token),
            SessionChange::Ended(ending) => format!("ended {ending:?}"),
        });
    }));
    (client, changes)
}

/// A deadlock must fail the test, not hang it.
async fn get(client: &Client) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(5), client.get::<Value>("/x")).await.expect("the request hung")
}

#[tokio::test(flavor = "multi_thread")]
async fn two_refusals_end_the_session() {
    for status in [400, 401, 422] {
        let server = script(&[(401, ""), (status, REFUSED), (status, REFUSED)]);
        let (client, changes) = holding_a_session(&server.base);
        assert!(matches!(get(&client).await, Err(Error::SessionEnded)), "status {status}");
        assert!(client.session().is_none());
        assert_eq!(*changes.lock(), ["ended Refused"]);
        assert_eq!(*server.seen.lock(), ["/x", REFRESH_PATH, REFRESH_PATH]);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_401_from_the_refresh_is_an_answer_and_does_not_hang() {
    let server = script(&[(401, ""), (401, ""), (401, "")]);
    let (client, _) = holding_a_session(&server.base);
    assert!(matches!(get(&client).await, Err(Error::SessionEnded)));
}

#[tokio::test(flavor = "multi_thread")]
async fn anything_but_a_refusal_keeps_the_session() {
    for status in [403, 404, 409, 500, 502, 504] {
        let server = script(&[(401, ""), (status, "")]);
        let (client, changes) = holding_a_session(&server.base);
        assert!(matches!(get(&client).await, Err(Error::Api { status: s, .. }) if s == status));
        assert!(client.session().is_some(), "status {status} ended the session");
        assert!(changes.lock().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rate_limits_and_overload_keep_the_session() {
    for status in [429, 503] {
        let server = script(&[(401, ""), (status, ""), (status, "")]);
        let (client, changes) = holding_a_session(&server.base);
        let error = get(&client).await.unwrap_err();
        assert_eq!(error.link(), Some(Link::Unavailable));
        assert!(client.session().is_some() && changes.lock().is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refusal_that_does_not_repeat_changes_nothing() {
    let server = script(&[(401, ""), (422, REFUSED), (200, NEW_TOKENS), (200, FINE)]);
    let (client, changes) = holding_a_session(&server.base);
    assert!(get(&client).await.is_ok());
    assert_eq!(client.session().unwrap().access_token, "new-access");
    assert_eq!(*changes.lock(), ["renewed new-access"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn trouble_during_the_second_look_is_no_verdict() {
    let server = script(&[(401, ""), (422, REFUSED), (500, "")]);
    let (client, changes) = holding_a_session(&server.base);
    assert!(matches!(get(&client).await, Err(Error::Api { status: 500, .. })));
    assert!(client.session().is_some() && changes.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn losing_the_network_during_the_second_look_is_no_verdict() {
    let server = script(&[(401, ""), (422, REFUSED)]);
    let (client, changes) = holding_a_session(&server.base);
    assert!(matches!(get(&client).await, Err(Error::Network(_))));
    assert!(client.session().is_some() && changes.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_is_not_read_as_a_refusal() {
    let server = script(&[(401, ""), (422, REFUSED)]);
    let (client, changes) = holding_a_session(&server.base);
    client.set_leaving(true);
    let started = std::time::Instant::now();
    assert!(matches!(get(&client).await, Err(Error::Cancelled)));
    assert!(started.elapsed() < REFUSAL_RECHECK, "a person leaving must not wait for a second look");
    assert!(changes.lock().is_empty());
    assert_eq!(server.seen.lock().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_during_the_second_look_is_not_read_as_a_refusal() {
    let server = script(&[(401, ""), (422, REFUSED)]);
    let (client, changes) = holding_a_session(&server.base);
    let request = tokio::spawn({
        let client = client.clone();
        async move { get(&client).await }
    });
    while server.seen.lock().len() < 2 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(REFUSAL_RECHECK / 4).await;
    client.set_leaving(true);
    assert!(matches!(request.await.unwrap(), Err(Error::Cancelled)));
    assert!(changes.lock().is_empty());
    assert_eq!(server.seen.lock().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lock_during_the_refusal_leaves_the_sealed_session_alone() {
    let holder: Arc<Mutex<Option<Arc<Client>>>> = Arc::default();
    let locking = holder.clone();
    let server = serve(move |_, index| {
        if index == 1 {
            locking.lock().as_ref().unwrap().drop_session();
        }
        [(401, ""), (422, REFUSED)].get(index).copied()
    });
    let (client, changes) = holding_a_session(&server.base);
    *holder.lock() = Some(client.clone());
    assert!(matches!(get(&client).await, Err(Error::Cancelled)));
    assert!(changes.lock().is_empty());
    assert_eq!(server.seen.lock().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_ends_once_however_many_requests_were_waiting() {
    let server = serve(|path, _| Some(if path == REFRESH_PATH { (422, REFUSED) } else { (401, "") }));
    let (client, changes) = holding_a_session(&server.base);
    let requests = (0..5).map(|_| get(&client));
    let results = futures::future::join_all(requests).await;
    assert!(results.iter().all(|result| matches!(result, Err(Error::SessionEnded))));
    assert_eq!(*changes.lock(), ["ended Refused"]);
    assert_eq!(server.seen.lock().iter().filter(|path| *path == REFRESH_PATH).count(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_refresh_code_on_an_ordinary_request_ends_nothing() {
    let server = script(&[(422, REFUSED)]);
    let (client, changes) = holding_a_session(&server.base);
    assert!(matches!(get(&client).await, Err(Error::Api { status: 422, code: 10013, .. })));
    assert!(client.session().is_some() && changes.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_request_after_a_new_token_is_only_an_error() {
    let server = script(&[(401, ""), (200, NEW_TOKENS), (401, "")]);
    let (client, changes) = holding_a_session(&server.base);
    assert!(matches!(get(&client).await, Err(Error::Api { status: 401, .. })));
    assert_eq!(*changes.lock(), ["renewed new-access"]);
}

/// Someone on the network answers for Proton. Tokens must not leave, and the person must be told
/// what this is rather than that the connection dropped.
#[tokio::test(flavor = "multi_thread")]
async fn a_false_server_gets_nothing_and_is_named_for_what_it_is() {
    let (address, reached) = crate::test_server::false_server("127.0.0.1");
    let (client, changes) = holding_a_session(&format!("https://{address}"));
    let error = get(&client).await.unwrap_err();
    assert!(matches!(error, Error::Untrusted));
    assert_eq!(error.link(), Some(Link::Untrusted));
    assert!(!reached.load(std::sync::atomic::Ordering::Relaxed), "a request reached the false server");
    assert!(client.session().is_some() && changes.lock().is_empty(), "this must not end the session");
}

/// A proxy set by anything else on the machine would sit in the connection and resolve names itself.
#[tokio::test(flavor = "multi_thread")]
async fn a_proxy_in_the_environment_is_ignored() {
    let proxy = serve(|_, _| Some((200, FINE)));
    let server = serve(|_, _| Some((200, FINE)));
    // SAFETY: the other tests only build clients, which read these under the same lock.
    unsafe {
        std::env::set_var("HTTP_PROXY", &proxy.base);
        std::env::set_var("ALL_PROXY", &proxy.base);
    }
    let (client, _) = holding_a_session(&server.base);
    // Safety: as above, the same test's own variables, taken away again.
    unsafe {
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("ALL_PROXY");
    }
    assert!(get(&client).await.is_ok());
    assert!(proxy.seen.lock().is_empty(), "the proxy was used");
    assert_eq!(server.seen.lock().len(), 1);
}

/// A redirect must not take the session's headers to another server.
#[tokio::test(flavor = "multi_thread")]
async fn does_not_follow_a_redirect() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let (first, second) = (TcpListener::bind("127.0.0.1:0").unwrap(), TcpListener::bind("127.0.0.1:0").unwrap());
    let (base, target) = (first.local_addr().unwrap(), second.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut stream, _) = first.accept().unwrap();
        let _ = stream.read(&mut [0; 4096]);
        let reply = format!("HTTP/1.1 302 Found\r\nLocation: http://{target}/\r\nContent-Length: 0\r\n\r\n");
        let _ = stream.write_all(reply.as_bytes());
    });
    second.set_nonblocking(true).unwrap();
    let client = Client::new(&format!("http://{base}")).unwrap();
    assert!(client.get::<Value>("/x").await.is_err());
    assert!(second.accept().is_err(), "the second server was contacted");
}

#[test]
fn errors_say_what_they_mean_for_the_connection() {
    let api = |status| Error::Api { status, code: 0, message: String::new() };
    assert_eq!(Error::Network(String::new()).link(), Some(Link::Offline));
    for status in [409, 429, 500, 503, 599] {
        assert_eq!(api(status).link(), Some(Link::Unavailable), "status {status}");
    }
    for status in [400, 401, 403, 404, 422] {
        assert_eq!(api(status).link(), None, "status {status}");
    }
    assert_eq!(Error::SessionEnded.link(), None);
    assert_eq!(Error::WrongPassword.link(), None);
}

#[test]
fn waiting_on_proton_is_slower_than_waiting_for_a_network() {
    assert_eq!(Link::Offline.retry_delay(), Duration::from_secs(5));
    assert_eq!(Link::Unavailable.retry_delay(), Duration::from_secs(30));
}

#[test]
fn the_page_learns_how_the_connection_stands_from_an_error() {
    let value = serde_json::to_value(Error::Network("down".into())).unwrap();
    assert_eq!(value["link"], "offline");
    let value = serde_json::to_value(Error::Api { status: 503, code: 0, message: "x".into() }).unwrap();
    assert_eq!(value["link"], "unavailable");
    let value = serde_json::to_value(Error::SessionEnded).unwrap();
    assert_eq!(value["kind"], "sessionEnded");
    assert!(value.get("link").is_none());
}

#[test]
fn endings_reach_the_page_by_name() {
    assert_eq!(serde_json::to_value(Ending::Refused).unwrap(), "refused");
    assert_eq!(serde_json::to_value(Ending::Chosen).unwrap(), "chosen");
}

#[test]
fn identifies_honestly() {
    assert!(user_agent().starts_with("proton-mail-light/"));
    assert!(user_agent().contains("unofficial"));
    assert!(user_agent().contains("Yaroslav Koval"));
    assert_eq!(APP_VERSION, "Other");
}

#[test]
fn retry_after_uses_the_header_or_a_default() {
    assert_eq!(retry_delay(Some("7")), 7.0);
    assert_eq!(retry_delay(Some("0")), 0.0);
    assert_eq!(retry_delay(Some("soon")), DEFAULT_RETRY_SECONDS);
    assert_eq!(retry_delay(Some("-1")), DEFAULT_RETRY_SECONDS);
    assert_eq!(retry_delay(None), DEFAULT_RETRY_SECONDS);
}

#[test]
fn accepts_code_1000_and_a_body_without_a_code() {
    assert!(check(200, &json!({"Code": 1000}), false).is_ok());
    assert!(check(200, &json!({}), false).is_ok());
}

#[test]
fn a_batch_code_counts_only_on_batch_endpoints() {
    assert!(check(200, &json!({"Code": 1001}), true).is_ok());
    assert!(check(200, &json!({"Code": 1001}), false).is_err());
}

#[test]
fn carries_proton_error_text_and_code() {
    let error = check(422, &json!({"Code": 8002, "Error": "Incorrect login"}), false).unwrap_err();
    assert!(matches!(error, Error::Api { status: 422, code: 8002, .. }));
    assert_eq!(error.to_string(), "Incorrect login");
}

#[test]
fn an_empty_error_body_still_says_something() {
    assert_eq!(check(503, &Value::Null, false).unwrap_err().to_string(), "HTTP 503");
}

#[test]
fn a_challenge_becomes_a_verification_step() {
    let body = json!({"Code": 9001, "Details": {
        "HumanVerificationToken": "t",
        "HumanVerificationMethods": ["captcha", "email"],
        "WebUrl": "https://verify.proton.me"
    }});
    let Err(Error::Verification(challenge)) = check(422, &body, false) else {
        panic!("expected a challenge");
    };
    assert_eq!(challenge.token, "t");
    assert_eq!(challenge.methods, ["captcha", "email"]);
}

#[test]
fn a_9001_without_a_token_is_an_ordinary_error() {
    assert!(matches!(check(422, &json!({"Code": 9001}), false), Err(Error::Api { .. })));
}

#[test]
fn prefers_captcha_when_several_methods_are_offered() {
    let client = Client::new(PROTON_API).unwrap();
    let challenge = |methods: &[&str]| HumanVerification {
        token: "t".into(),
        methods: methods.iter().map(std::string::ToString::to_string).collect(),
        web_url: String::new(),
    };
    client.set_verification(&challenge(&["email", "captcha"]));
    assert_eq!(client.verification.lock().as_ref().unwrap().method, "captcha");
    client.set_verification(&challenge(&["email"]));
    assert_eq!(client.verification.lock().as_ref().unwrap().method, "email");
}

#[test]
fn finds_the_first_failed_batch_item() {
    let ok = json!({"Responses": [{"Response": {"Code": 1000}}]});
    assert_eq!(first_batch_failure(&ok), None);
    let bad = json!({"Responses": [
        {"Response": {"Code": 1000}},
        {"Response": {"Code": 2501, "Error": "Message does not exist"}}
    ]});
    assert_eq!(first_batch_failure(&bad).as_deref(), Some("Message does not exist"));
}

#[test]
fn a_failed_item_without_text_shows_its_code() {
    let bad = json!({"Responses": [{"Response": {"Code": 2501}}]});
    assert_eq!(first_batch_failure(&bad).as_deref(), Some("code 2501"));
}

#[test]
fn serialises_for_the_interface() {
    let error = Error::Api { status: 422, code: 8002, message: "Bad".into() };
    let value = serde_json::to_value(error).unwrap();
    assert_eq!(value["kind"], "api");
    assert_eq!(value["code"], 8002);
    assert_eq!(value["message"], "Bad");
}

fn signed_in_at(base: &str) -> Client {
    let client = Client::new(base).unwrap();
    client.set_session(Some(Session {
        uid: "uid".into(),
        access_token: "access".into(),
        refresh_token: "refresh".into(),
        password_mode: 1,
    }));
    client
}

#[tokio::test(flavor = "multi_thread")]
async fn a_settings_tab_asks_proton_once() {
    let server = crate::test_server::serve(|_, _| Some((200, r#"{"Code":1000,"Answer":1}"#)));
    let client = signed_in_at(&server.base);
    for _ in 0..3 {
        let body: Value = client.get("/core/v4/settings").await.unwrap();
        assert_eq!(body["Answer"], 1);
    }
    assert_eq!(server.seen.lock().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_makes_settings_be_read_again() {
    let server = crate::test_server::serve(|_, _| Some((200, r#"{"Code":1000}"#)));
    let client = signed_in_at(&server.base);
    let _: Value = client.get("/core/v4/settings").await.unwrap();
    client.act(Method::PUT, "/core/v4/settings/weekstart").await.unwrap();
    let _: Value = client.get("/core/v4/settings").await.unwrap();
    assert_eq!(server.seen.lock().len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn work_on_mail_keeps_the_settings_read() {
    let server = crate::test_server::serve(|_, _| Some((200, r#"{"Code":1000}"#)));
    let client = signed_in_at(&server.base);
    let _: Value = client.get("/core/v4/settings").await.unwrap();
    client.act(Method::PUT, "/mail/v4/messages/read").await.unwrap();
    let _: Value = client.get("/core/v4/settings").await.unwrap();
    assert_eq!(server.seen.lock().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_kept_outlives_the_session() {
    let server = crate::test_server::serve(|_, _| Some((200, r#"{"Code":1000}"#)));
    let client = signed_in_at(&server.base);
    let _: Value = client.get("/core/v4/settings").await.unwrap();
    client.drop_session();
    assert!(client.kept.lock().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_setting_the_plan_lacks_is_asked_once() {
    let server = crate::test_server::serve(|_, _| Some((403, r#"{"Code":2026,"Error":"Not allowed"}"#)));
    let client = signed_in_at(&server.base);
    for _ in 0..3 {
        let refused = client.get::<Value>("/core/v4/domains?Page=0").await;
        assert!(matches!(refused, Err(Error::Api { status: 403, code: 2026, .. })));
    }
    assert_eq!(server.seen.lock().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_setting_is_asked_again() {
    let server = crate::test_server::serve(|_, _| Some((422, r#"{"Code":2001,"Error":"Invalid"}"#)));
    let client = signed_in_at(&server.base);
    for _ in 0..2 {
        assert!(client.get::<Value>("/core/v4/domains?Page=0").await.is_err());
    }
    assert_eq!(server.seen.lock().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn mail_itself_is_never_kept_here() {
    let server = crate::test_server::serve(|_, _| Some((200, r#"{"Code":1000}"#)));
    let client = signed_in_at(&server.base);
    for _ in 0..2 {
        let _: Value = client.get("/mail/v4/messages?LabelID=0").await.unwrap();
    }
    assert_eq!(server.seen.lock().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_with_no_way_chosen_sends_nothing() {
    let server = serve(|_, _| Some((200, FINE)));
    let client = Client::undecided(&server.base);
    assert!(matches!(client.get::<Value>("/x").await, Err(Error::Network(_))));
    assert!(server.seen.lock().is_empty());
}

#[test]
fn the_way_is_chosen_once_and_kept() {
    let client = Client::new("http://127.0.0.1:9").unwrap();
    assert!(client.decide(&Way::Direct).is_err());
    assert!(!client.through_tor());
}

/// The test that matters: with Tor chosen and Tor down, a server that is right there is not reached at all.
#[tokio::test(flavor = "multi_thread")]
async fn with_tor_down_nothing_goes_out_another_way() {
    let server = serve(|_, _| Some((200, FINE)));
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_door = format!("socks5h://u:p@{}", closed.local_addr().unwrap());
    drop(closed);
    let dns = Arc::new(Quad9::new().unwrap());
    let tls = crate::tls::pinned_config(crate::tls::PROTON_PINS).unwrap();
    let http = http_client_with(&server.base, tls, Some(&dead_door), &dns).unwrap();
    let answer = http.get(format!("{}/x", server.base)).send().await;
    assert!(answer.is_err());
    assert!(server.seen.lock().is_empty(), "the request went around Tor");
}

#[tokio::test(flavor = "multi_thread")]
async fn under_tor_no_name_is_looked_up_here() {
    use reqwest::dns::Resolve;
    let refused = crate::dns::NoLookups.resolve("mail-api.proton.me".parse().unwrap()).await;
    assert!(refused.is_err());
}

/// A check clicks anywhere: whatever it asks but a read must never reach Proton, and the session must stay.
#[tokio::test(flavor = "multi_thread")]
async fn a_check_changes_nothing_and_reads_still_work() {
    let server = serve(|_, _| Some((200, FINE)));
    let (client, _) = holding_a_session(&server.base);
    client.forbid_changes();
    let requests = client.record_requests();
    assert!(matches!(
        client.put::<Value>("/mail/v4/messages/read", json!({"IDs": ["a"]})).await,
        Err(Error::RefusedInCheck)
    ));
    assert!(matches!(client.post::<Value>("/mail/v4/messages/send", json!({})).await, Err(Error::RefusedInCheck)));
    assert!(matches!(client.act(Method::DELETE, "/auth/v4").await, Err(Error::RefusedInCheck)));
    assert!(server.seen.lock().is_empty(), "a change reached the server: {:?}", server.seen.lock());
    assert!(get(&client).await.is_ok(), "a read must still go");
    assert_eq!(*server.seen.lock(), ["/x"]);
    assert!(client.session().is_some(), "a refused change ended the session");
    let summary = requests.summary();
    assert_eq!((summary.total, summary.refused), (4, 3));
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_check_changes_go_and_nothing_is_recorded() {
    let server = serve(|_, _| Some((200, FINE)));
    let (client, _) = holding_a_session(&server.base);
    assert!(client.put::<Value>("/mail/v4/messages/read", json!({"IDs": ["a"]})).await.is_ok());
    assert_eq!(*server.seen.lock(), ["/mail/v4/messages/read"]);
    assert!(!client.changes_forbidden());
}
