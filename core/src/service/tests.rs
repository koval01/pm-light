use super::*;
use crate::model::SenderList;
use crate::test_server::{Server, script, serve};

#[test]
fn only_plain_image_types_become_data_urls() {
    assert!(is_inline_image("image/png"));
    assert!(is_inline_image("image/svg+xml"));
    assert!(!is_inline_image("text/html"));
    assert!(!is_inline_image("image/"));
    assert!(!is_inline_image("image/png;x"));
    assert!(!is_inline_image("abcde\u{20ac}gif"));
}

#[test]
fn body_text_normalises_line_ends_and_nbsp() {
    assert_eq!(body_text("a\r\nb\u{a0}c".as_bytes()), "a\nb c");
}

#[test]
fn query_values_are_encoded() {
    assert_eq!(encoded("a+b@x.io"), "a%2Bb%40x.io");
}

#[test]
fn progress_steps_serialise_for_the_interface() {
    assert_eq!(serde_json::to_value(AuthStep::Totp).unwrap()["step"], "totp");
    let verify = AuthStep::Verify {
        challenge: HumanVerification { token: "t".into(), methods: vec![], web_url: "u".into() },
    };
    let value = serde_json::to_value(verify).unwrap();
    assert_eq!(value["step"], "verify");
    assert_eq!(value["challenge"]["webUrl"], "u");
}

#[derive(Default)]
struct Memory(Mutex<Option<String>>, Mutex<Option<String>>);

impl SessionStore for Memory {
    fn load(&self) -> Option<String> {
        self.0.lock().clone()
    }
    fn save(&self, value: &str) {
        *self.0.lock() = Some(value.to_string());
    }
    fn clear(&self) {
        *self.0.lock() = None;
    }
    fn load_attempts(&self) -> Option<String> {
        self.1.lock().clone()
    }
    fn save_attempts(&self, value: &str) {
        *self.1.lock() = Some(value.to_string());
    }
    fn clear_attempts(&self) {
        *self.1.lock() = None;
    }
}

fn signed_in() -> (Arc<Service>, Arc<Memory>) {
    let store = Arc::new(Memory::default());
    let service = Service::new("http://127.0.0.1:9", store.clone(), Box::new(|_| {})).unwrap();
    *service.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
    service.client.set_session(Some(Session {
        uid: "uid".into(),
        access_token: "access".into(),
        refresh_token: "refresh-token".into(),
        password_mode: 1,
    }));
    (service, store)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap()
}

#[test]
fn without_a_password_the_session_is_saved_as_is() {
    let (_, store) = signed_in();
    assert!(store.load().unwrap().contains("refresh-token"));
}

#[test]
fn an_app_password_seals_what_is_saved() {
    let (service, store) = signed_in();
    runtime().block_on(service.set_app_password("1234")).unwrap();
    let saved = store.load().unwrap();
    assert!(!saved.contains("refresh-token") && !saved.contains("access"));
    assert!(service.app_lock_enabled());
}

#[test]
fn refreshed_tokens_stay_sealed() {
    let (service, store) = signed_in();
    runtime().block_on(service.set_app_password("1234")).unwrap();
    service.client.set_session(Some(Session {
        uid: "uid".into(),
        access_token: "new".into(),
        refresh_token: "newer-token".into(),
        password_mode: 1,
    }));
    assert!(!store.load().unwrap().contains("newer-token"));
}

#[test]
fn locking_empties_memory_and_keeps_the_sealed_session() {
    let (service, store) = signed_in();
    runtime().block_on(service.set_app_password("1234")).unwrap();
    assert!(service.lock_now());
    assert!(service.client.session().is_none() && service.key_secret.lock().is_none());
    assert!(service.lock_key().is_none());
    assert!(store.load().is_some());
}

#[test]
fn locking_does_nothing_without_a_password() {
    let (service, _) = signed_in();
    assert!(!service.lock_now());
    assert!(service.client.session().is_some());
}

#[test]
fn a_locked_app_stops_at_the_password() {
    let (service, _) = signed_in();
    let runtime = runtime();
    runtime.block_on(service.set_app_password("1234")).unwrap();
    service.lock_now();
    assert_eq!(runtime.block_on(service.restore()).unwrap(), AuthStep::Locked);
}

#[test]
fn a_wrong_app_password_changes_nothing() {
    let (service, store) = signed_in();
    let runtime = runtime();
    runtime.block_on(service.set_app_password("1234")).unwrap();
    service.lock_now();
    let before = store.load();
    assert!(matches!(runtime.block_on(service.unlock_app("0000")), Err(Error::WrongPassword)));
    assert_eq!(store.load(), before);
    assert!(service.lock_key().is_none());
}

fn locked_with_failures(failures: usize) -> (Arc<Service>, Arc<Memory>, tokio::runtime::Runtime) {
    let (service, store) = signed_in();
    let runtime = runtime();
    runtime.block_on(service.set_app_password("1234")).unwrap();
    service.lock_now();
    for _ in 0..failures {
        assert!(matches!(runtime.block_on(service.unlock_app("0000")), Err(Error::WrongPassword)));
    }
    (service, store, runtime)
}

#[test]
fn too_many_wrong_passwords_freeze_even_the_right_one() {
    let (service, _, runtime) = locked_with_failures(5);
    let Err(Error::TooManyAttempts { seconds }) = runtime.block_on(service.unlock_app("1234")) else {
        panic!("the app should be frozen");
    };
    assert!((1..=30).contains(&seconds));
    assert!(service.client.session().is_none());
}

#[test]
fn a_restart_does_not_lift_the_freeze() {
    let (_, store, _) = locked_with_failures(5);
    let restarted = Service::new("http://127.0.0.1:9", store, Box::new(|_| {})).unwrap();
    assert!(restarted.unlock_wait() > 0);
}

#[test]
fn the_right_password_clears_the_count() {
    let (service, store, runtime) = locked_with_failures(4);
    assert!(store.load_attempts().is_some());
    assert!(matches!(runtime.block_on(service.unlock_app("1234")), Err(Error::Network(_))));
    assert_eq!(store.load_attempts(), None);
}

#[test]
fn a_frozen_app_can_still_open_with_the_key_from_touch_id() {
    let (service, store, runtime) = locked_with_failures(5);
    let envelope: vault::Envelope = serde_json::from_str(&store.load().unwrap()).unwrap();
    let key = vault::derive_key("1234", &vault::salt_of(&envelope).unwrap()).unwrap();
    assert!(matches!(runtime.block_on(service.unlock_with_key(key)), Err(Error::Network(_))));
    assert_eq!(service.unlock_wait(), 0);
}

#[test]
fn forgetting_the_session_lifts_the_freeze() {
    let (service, store, _) = locked_with_failures(5);
    service.forget_locked_session();
    assert_eq!(service.unlock_wait(), 0);
    assert_eq!(store.load_attempts(), None);
}

#[test]
fn nothing_saved_after_locking_leaves_the_session_unsealed() {
    let (service, store) = signed_in();
    let session = service.client.session().unwrap();
    runtime().block_on(service.set_app_password("1234")).unwrap();
    service.lock_now();
    service.persist(&session);
    assert!(!store.load().unwrap().contains("refresh-token"));
}

#[test]
fn the_right_app_password_reaches_the_server_step() {
    let (service, _) = signed_in();
    let runtime = runtime();
    runtime.block_on(service.set_app_password("1234")).unwrap();
    service.lock_now();
    // Nothing listens on this port, so the session is opened and only the network is missing.
    assert!(matches!(runtime.block_on(service.unlock_app("1234")), Err(Error::Network(_))));
    assert!(service.client.session().is_some());
}

#[test]
fn removing_the_password_needs_the_password() {
    let (service, store) = signed_in();
    let runtime = runtime();
    runtime.block_on(service.set_app_password("1234")).unwrap();
    assert!(runtime.block_on(service.remove_app_password("wrong")).is_err());
    assert!(!store.load().unwrap().contains("refresh-token"));
    runtime.block_on(service.remove_app_password("1234")).unwrap();
    assert!(store.load().unwrap().contains("refresh-token"));
}

#[test]
fn a_session_ended_elsewhere_leaves_nothing_behind() {
    let (service, store) = signed_in();
    runtime().block_on(service.set_app_password("1234")).unwrap();
    service.client.set_session(None);
    assert!(store.load().is_none());
    assert!(service.key_secret.lock().is_none() && service.lock_key().is_none());
}

/// The app starting again with a saved session, against whatever `base` answers.
fn restarted_at(base: &str) -> (Arc<Service>, Arc<Memory>, Arc<Mutex<Vec<Ending>>>) {
    let (_, store) = signed_in();
    let endings = Arc::new(Mutex::new(Vec::new()));
    let record = endings.clone();
    let service = Service::new(base, store.clone(), Box::new(move |ending| record.lock().push(ending))).unwrap();
    (service, store, endings)
}

fn take_up_saved_session(service: &Service, store: &Memory) {
    let saved: SavedSession = serde_json::from_str(&store.load().unwrap()).unwrap();
    service.client.set_session(Some(saved.session));
}

const REFUSED: &str = r#"{"Code":10013,"Error":"Invalid refresh token"}"#;

/// Every request meets an expired token, and Proton refuses to renew it.
fn refusing() -> Server {
    serve(|path, _| Some(if path == "/auth/v4/refresh" { (422, REFUSED) } else { (401, "") }))
}

#[test]
fn a_bad_minute_at_the_start_keeps_the_saved_session() {
    for status in [409, 429, 500, 502, 503] {
        let server = serve(move |_, _| Some((status, "")));
        let (service, store, endings) = restarted_at(&server.base);
        let saved = store.load();
        let step = runtime().block_on(service.reopen());
        assert_eq!(step, AuthStep::Waiting { link: Link::Unavailable }, "status {status}");
        assert_eq!(store.load(), saved);
        assert!(endings.lock().is_empty());
    }
}

#[test]
fn a_reply_that_makes_no_sense_at_the_start_keeps_the_saved_session() {
    let server = serve(|_, _| Some((200, "<html>maintenance</html>")));
    let (service, store, endings) = restarted_at(&server.base);
    let saved = store.load();
    let step = runtime().block_on(service.reopen());
    assert_eq!(step, AuthStep::Waiting { link: Link::Unavailable });
    assert_eq!(store.load(), saved);
    assert!(endings.lock().is_empty());
}

#[test]
fn no_network_at_the_start_waits_for_one() {
    let (service, store, _) = restarted_at("http://127.0.0.1:9");
    assert_eq!(runtime().block_on(service.reopen()), AuthStep::Waiting { link: Link::Offline });
    assert!(store.load().is_some());
}

#[test]
fn refused_at_the_start_means_signing_in_and_saying_why() {
    let server = refusing();
    let (service, store, endings) = restarted_at(&server.base);
    assert_eq!(runtime().block_on(service.reopen()), AuthStep::Credentials);
    assert!(store.load().is_none());
    assert_eq!(*endings.lock(), [Ending::Refused]);
}

#[test]
fn a_session_refused_while_running_leaves_nothing_behind() {
    let server = refusing();
    let (service, store, endings) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    *service.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
    assert!(matches!(runtime().block_on(service.check_mail()), Err(Error::SessionEnded)));
    assert!(store.load().is_none() && service.key_secret.lock().is_none());
    assert!(!service.is_signed_in());
    assert_eq!(*endings.lock(), [Ending::Refused]);
}

#[test]
fn signing_out_is_a_choice() {
    let server = script(&[(200, r#"{"Code":1000}"#)]);
    let (service, store, endings) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    runtime().block_on(service.sign_out()).unwrap();
    assert!(store.load().is_none());
    assert_eq!(*endings.lock(), [Ending::Chosen]);
}

#[test]
fn signing_out_offline_still_ends_it_here_as_a_choice() {
    let (service, store, endings) = restarted_at("http://127.0.0.1:9");
    take_up_saved_session(&service, &store);
    assert!(runtime().block_on(service.sign_out()).is_err());
    assert!(store.load().is_none());
    assert_eq!(*endings.lock(), [Ending::Chosen]);
}

#[test]
fn a_bad_minute_during_unlock_keeps_the_sealed_session() {
    let (service, store) = signed_in();
    runtime().block_on(service.set_app_password("1234")).unwrap();
    let server = serve(|_, _| Some((503, "")));
    let restarted = Service::new(&server.base, store.clone(), Box::new(|_| {})).unwrap();
    let sealed = store.load();
    let error = runtime().block_on(restarted.unlock_app("1234")).unwrap_err();
    assert_eq!(error.link(), Some(Link::Unavailable));
    assert_eq!(store.load(), sealed);
}

#[test]
fn refused_during_unlock_means_signing_in() {
    let (service, store) = signed_in();
    runtime().block_on(service.set_app_password("1234")).unwrap();
    let server = refusing();
    let endings = Arc::new(Mutex::new(Vec::new()));
    let record = endings.clone();
    let restarted = Service::new(&server.base, store.clone(), Box::new(move |e| record.lock().push(e))).unwrap();
    assert_eq!(runtime().block_on(restarted.unlock_app("1234")).unwrap(), AuthStep::Credentials);
    assert!(store.load().is_none());
    assert_eq!(*endings.lock(), [Ending::Refused]);
}

#[test]
fn billing_keeps_what_it_could_read() {
    let server = serve(|path, _| {
        Some(match path.split('?').next().unwrap_or_default() {
            "/core/v4/users" => (200, r#"{"Code":1000,"User":{"Subscribed":1,"Credit":5,"Currency":"EUR"}}"#),
            "/payments/v4/subscription" => {
                (200, r#"{"Code":1000,"Subscription":{"Cycle":12,"Plans":[{"Type":1,"Title":"Mail Plus"}]}}"#)
            }
            "/payments/v5/invoices" => (200, r#"{"Code":1000,"Invoices":[{"ID":"1","State":1}]}"#),
            _ => (500, ""),
        })
    });
    let (service, store, _) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    let billing = runtime().block_on(service.billing()).unwrap();
    assert_eq!(billing.plan.unwrap().title, "Mail Plus");
    assert!(billing.methods.is_empty());
    assert_eq!(billing.invoices.len(), 1);
}

#[test]
fn a_free_account_is_not_asked_about_a_subscription() {
    let server = serve(|path, _| {
        Some(if path == "/core/v4/users" { (200, r#"{"Code":1000,"User":{"Subscribed":0}}"#) } else { (200, "{}") })
    });
    let (service, store, _) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    assert!(runtime().block_on(service.billing()).unwrap().plan.is_none());
    assert!(!server.seen.lock().iter().any(|path| path.contains("subscription")));
}

#[test]
fn waiting_reaches_the_page_with_what_it_waits_for() {
    let value = serde_json::to_value(AuthStep::Waiting { link: Link::Unavailable }).unwrap();
    assert_eq!(value["step"], "waiting");
    assert_eq!(value["link"], "unavailable");
}

#[test]
fn being_offline_at_start_keeps_the_saved_session() {
    let (service, store) = signed_in();
    let saved = store.load();
    service.client.drop_session();
    assert!(matches!(runtime().block_on(service.restore()), Err(Error::Network(_))));
    assert_eq!(store.load(), saved);
}

#[test]
fn sender_rules_read_every_page() {
    let server = serve(|path, _| {
        let full: String = (0..100)
            .map(|i| format!(r#"{{"ID":"{i}","Location":14,"Email":"a{i}@b.c"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let full: &'static str = Box::leak(format!(r#"{{"Code":1000,"IncomingDefaults":[{full}]}}"#).into_boxed_str());
        Some(if path.contains("Page=0") { (200, full) } else {
            (200, r#"{"Code":1000,"IncomingDefaults":[{"ID":"x","Location":0,"Domain":"b.c"}]}"#)
        })
    });
    let (service, store, _) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    assert_eq!(runtime().block_on(service.sender_rules()).unwrap().len(), 101);
    assert_eq!(server.seen.lock().len(), 2);
}

#[test]
fn a_mistyped_sender_never_reaches_proton() {
    let server = script(&[]);
    let (service, store, _) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    assert!(runtime().block_on(service.add_sender_rule("nonsense", SenderList::Blocked)).is_err());
    assert!(server.seen.lock().is_empty());
}

#[test]
fn forwarding_ids_cannot_reach_another_path() {
    let server = serve(|_, _| Some((200, r#"{"Code":1000}"#)));
    let (service, store, _) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    runtime().block_on(service.pause_forwarding("../../core/v4/users", true)).unwrap();
    runtime().block_on(service.pause_forwarding("abc", false)).unwrap();
    let seen = server.seen.lock();
    assert!(seen[0].starts_with("/mail/v4/forwardings/") && !seen[0].contains("/../"));
    assert_eq!(seen[1], "/mail/v4/forwardings/abc/resume");
}

#[test]
fn a_domain_this_account_does_not_hold_is_not_looked_up() {
    let server = serve(|_, _| Some((200, r#"{"Code":1000,"Domains":[{"DomainName":"own.example"}]}"#)));
    let (service, store, _) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    assert!(matches!(runtime().block_on(service.check_domain("other.example")), Err(Error::Other(_))));
}

/// On a thread of its own: code that freezes must fail this test, not take the whole run with it.
pub(super) fn finishes<T: Send + 'static>(work: impl Future<Output = T> + Send + 'static) -> T {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(runtime().block_on(work));
    });
    finished.recv_timeout(std::time::Duration::from_secs(10)).expect("it froze")
}

fn a_page_of_mail(rows: usize) -> &'static str {
    let rows = (0..rows).map(|id| format!(r#"{{"ID":"{id}"}}"#)).collect::<Vec<_>>().join(",");
    format!(r#"{{"Code":1000,"Messages":[{rows}]}}"#).leak()
}

pub(super) fn mailbox_at(base: &str) -> (Arc<Service>, Arc<Mutex<Vec<crate::view::ViewUpdate>>>) {
    let (service, store, _) = restarted_at(base);
    take_up_saved_session(&service, &store);
    let updates = Arc::new(Mutex::new(Vec::new()));
    let record = updates.clone();
    service.on_view(Box::new(move |update| record.lock().push(update.clone())));
    (service, updates)
}

pub(super) fn rows_shown(updates: &Mutex<Vec<crate::view::ViewUpdate>>) -> usize {
    updates.lock().iter().rev().find_map(|update| update.rows.as_ref()).map_or(0, Vec::len)
}

/// The first thing every start does: this froze the app on its splash with mail on the way.
#[test]
fn opening_a_folder_shows_its_rows() {
    let server = serve(|_, _| Some((200, a_page_of_mail(3))));
    let (service, updates) = mailbox_at(&server.base);
    let opening = service;
    finishes(async move { opening.view_open("inbox").await });
    assert_eq!(rows_shown(&updates), 3);
    assert!(!updates.lock().last().unwrap().loading);
}

#[test]
fn reloading_filtering_and_paging_all_finish() {
    let server = serve(|_, _| Some((200, a_page_of_mail(50))));
    let (service, updates) = mailbox_at(&server.base);
    let working = service;
    finishes(async move {
        working.view_open("inbox").await;
        working.view_reload().await;
        let unread = crate::view::Filters { unread: true, ..Default::default() };
        working.view_filter(unread).await;
        working.view_more().await;
    });
    let lists = server.seen.lock().iter().filter(|path| path.starts_with("/mail/v4/messages?")).count();
    assert_eq!(lists, 4);
    assert_eq!(rows_shown(&updates), 50);
}

#[test]
fn a_folder_that_cannot_load_says_so_instead_of_loading_forever() {
    let server = script(&[(500, r#"{"Code":2000,"Error":"down"}"#)]);
    let (service, updates) = mailbox_at(&server.base);
    let opening = service;
    finishes(async move { opening.view_open("inbox").await });
    let last = updates.lock().last().unwrap().clone();
    assert!(!last.loading);
    assert!(last.error.is_some());
}

fn with_stages(service: &Service) -> Arc<Mutex<Vec<Stage>>> {
    let stages = Arc::new(Mutex::new(Vec::new()));
    let record = stages.clone();
    service.on_stage(Box::new(move |stage| record.lock().push(stage)));
    stages
}

#[test]
fn a_start_names_each_stage_as_it_reaches_it() {
    let server = serve(|path, _| {
        Some((200, if path == "/core/v4/users" {
            r#"{"Code":1000,"User":{"Keys":[]}}"#
        } else {
            r#"{"Code":1000,"Addresses":[]}"#
        }))
    });
    let (service, _, _) = restarted_at(&server.base);
    let stages = with_stages(&service);
    let _ = runtime().block_on(service.restore());
    assert_eq!(*stages.lock(), [Stage::Session, Stage::Connecting, Stage::Keys]);
}

#[test]
fn a_start_with_nothing_saved_stops_at_the_keychain() {
    let service = Service::new("http://127.0.0.1:9", Arc::new(Memory::default()), Box::new(|_| {})).unwrap();
    let stages = with_stages(&service);
    assert_eq!(runtime().block_on(service.restore()).unwrap(), AuthStep::Credentials);
    assert_eq!(*stages.lock(), [Stage::Session]);
}

#[test]
fn a_start_that_cannot_reach_proton_stays_on_connecting() {
    let server = script(&[]);
    let (service, _, _) = restarted_at(&server.base);
    let stages = with_stages(&service);
    assert!(matches!(runtime().block_on(service.reopen()), AuthStep::Waiting { .. }));
    assert_eq!(*stages.lock(), [Stage::Session, Stage::Connecting]);
}

const A_PLAIN_MESSAGE: &str = r#"{"Code":1000,"Message":{"ID":"0","Body":"","MIMEType":"text/plain"}}"#;

fn a_kept_message(id: &str) -> Arc<Message> {
    Arc::new(Message {
        meta: MessageMeta {
            id: id.to_string(),
            folder: "inbox".into(),
            from: crate::model::Address { name: None, email: String::new() },
            to: Vec::new(),
            subject: String::new(),
            snippet: String::new(),
            date: 0,
            unread: true,
            starred: false,
            labels: Vec::new(),
            attachment_count: 0,
            conversation_id: String::new(),
            messages: 0,
            expires: 0,
        },
        address_id: String::new(),
        size: 0,
        signature: crate::crypto::SignatureStatus::Unsigned,
        raw_headers: String::new(),
        unsubscribe: None,
        receipt_requested: false,
        cc: Vec::new(),
        bcc: Vec::new(),
        reply_to: Vec::new(),
        body: "text".into(),
        safe_body: String::new(),
        protection: MailProtection::default(),
        details: crate::details::details_of(""),
        html: false,
        attachments: Vec::new(),
        from_proton: Some(false),
    })
}

fn with_a_picture(folder: &str, from_proton: bool) -> Message {
    let mut message = (*a_kept_message("picture")).clone();
    message.html = true;
    message.safe_body = r#"<img src="data:image/png;base64,AAAA">"#.into();
    message.meta.folder = folder.into();
    message.from_proton = Some(from_proton);
    message
}

#[test]
fn embedded_pictures_stay_until_the_account_hides_them() {
    let (service, _) = signed_in();
    assert_eq!(service.with_embedded_held_back(&with_a_picture("inbox", false)), None);
}

#[test]
fn the_account_hides_embedded_pictures_but_not_in_drafts_or_from_proton() {
    let (service, _) = signed_in();
    *service.habits.write() = Habits { hide_embedded: true, ..Habits::default() };
    let held = service.with_embedded_held_back(&with_a_picture("inbox", false));
    assert!(held.is_some_and(|page| !page.contains("data:image")));
    assert_eq!(service.with_embedded_held_back(&with_a_picture("drafts", false)), None);
    assert_eq!(service.with_embedded_held_back(&with_a_picture("inbox", true)), None);
}

#[test]
fn mail_stored_before_the_sender_was_kept_is_asked_for_again_when_it_matters() {
    let (service, _) = signed_in();
    let mut old = (*a_kept_message("old")).clone();
    old.from_proton = None;
    assert!(!service.must_ask_again(&old));
    *service.habits.write() = Habits { hide_embedded: true, ..Habits::default() };
    assert!(service.must_ask_again(&old));
    assert!(!service.must_ask_again(&a_kept_message("new")));
}

#[test]
fn mail_stored_without_the_sender_reads_as_not_known() {
    let mut stored = serde_json::to_value(&*a_kept_message("old")).unwrap();
    stored.as_object_mut().unwrap().remove("fromProton");
    let message: Message = serde_json::from_value(stored).unwrap();
    assert_eq!(message.from_proton, None);
}

#[test]
fn a_message_read_once_costs_no_second_request() {
    let server = script(&[]);
    let (service, _) = mailbox_at(&server.base);
    service.keep_message(a_kept_message("known"));
    let kept = runtime().block_on(service.message("known")).unwrap();
    assert_eq!(kept.body, "text");
    assert!(server.seen.lock().is_empty());
}

#[test]
fn the_memory_of_messages_has_a_limit_in_bytes() {
    let (service, _) = signed_in();
    let mut large = (*a_kept_message("large")).clone();
    large.body = "x".repeat(OPENED_BYTES);
    service.keep_message(Arc::new(large));
    service.keep_message(a_kept_message("small"));
    let kept: Vec<String> = service.opened.lock().iter().map(|m| m.meta.id.clone()).collect();
    assert_eq!(kept, ["small"]);
}

#[test]
fn a_message_finished_after_a_lock_is_not_kept() {
    let (service, _) = signed_in();
    service.client.drop_session();
    service.keep_message(a_kept_message("late"));
    assert!(service.opened.lock().is_empty());
}

#[test]
fn a_saved_draft_is_read_again() {
    let (service, _) = signed_in();
    service.keep_message(a_kept_message("draft"));
    service.forget_message("draft");
    assert!(service.opened.lock().is_empty());
}

#[test]
fn a_message_of_which_the_files_are_gone_is_read_again() {
    let (service, _) = signed_in();
    let mut message = (*a_kept_message("mime")).clone();
    let file = crate::model::Attachment {
        id: "mime-0".into(),
        name: String::new(),
        size: 0,
        mime: String::new(),
        key_packets: String::new(),
        signed: false,
        inline: false,
        content_id: None,
        previewable: false,
    };
    message.attachments.push(file);
    service.keep_message(Arc::new(message));
    assert!(service.kept_message("mime").is_none());
}

#[test]
fn a_lock_takes_kept_mail_and_folders_along() {
    let (service, _) = signed_in();
    service.keep_message(a_kept_message("known"));
    service.view.lock().confirm(1);
    service.view.lock().open("archive");
    service.wipe_memory();
    assert!(service.opened.lock().is_empty());
    assert_eq!(service.view.lock().open("inbox"), 1);
    assert!(service.view.lock().update(false, Vec::new(), Vec::new()).updated_at.is_none());
}

#[test]
fn a_folder_comes_back_at_once_and_is_then_asked_again() {
    let server = serve(|_, _| Some((200, a_page_of_mail(2))));
    let (service, updates) = mailbox_at(&server.base);
    let working = service;
    finishes(async move {
        working.view_open("archive").await;
        working.view_open("sent").await;
        working.view_open("archive").await;
    });
    let back = updates.lock().iter().rev().find(|update| update.folder == "archive" && update.refreshing).cloned().unwrap();
    assert_eq!(back.rows.as_ref().map(Vec::len), Some(2), "the rows came before the answer");
    assert!(back.updated_at.is_some());
    let lists = server.seen.lock().iter().filter(|path| path.starts_with("/mail/v4/messages?")).count();
    assert_eq!(lists, 3);
}

#[test]
fn a_failed_refresh_keeps_what_is_on_screen() {
    let server = serve(|_, index| Some(if index == 0 { (200, a_page_of_mail(2)) } else { (500, "{}") }));
    let (service, updates) = mailbox_at(&server.base);
    let working = service;
    finishes(async move {
        working.view_open("archive").await;
        working.view_reload().await;
    });
    let last = updates.lock().last().unwrap().clone();
    assert!(last.error.is_some());
    assert!(last.updated_at.is_some());
    assert_eq!(rows_shown(&updates), 2);
}

#[test]
fn spam_is_not_read_ahead() {
    let server = serve(|_, _| Some((200, a_page_of_mail(3))));
    let (service, _) = mailbox_at(&server.base);
    let working = service;
    finishes(async move {
        working.view_open("spam").await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    });
    assert_eq!(server.seen.lock().len(), 1);
}

#[test]
fn the_inbox_is_read_ahead_only_from_the_top() {
    let server = serve(|path, _| Some((200, if path.contains('?') { a_page_of_mail(9) } else { A_PLAIN_MESSAGE })));
    let (service, _) = mailbox_at(&server.base);
    let working = service;
    finishes(async move {
        working.view_open("inbox").await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    });
    let reads = server.seen.lock().iter().filter(|path| path.starts_with("/mail/v4/messages/")).count();
    assert_eq!(reads, 5);
}

#[test]
fn latency_says_nothing_rather_than_a_time_for_no_answer() {
    let (service, _) = signed_in();
    let latency = runtime().block_on(service.latency());
    assert_eq!(latency.proton, None);
}

#[test]
fn time_settings_go_to_their_own_paths_and_refuse_nonsense() {
    let server = serve(|_, _| Some((200, r#"{"Code":1000}"#)));
    let (service, store, _) = restarted_at(&server.base);
    take_up_saved_session(&service, &store);
    let working = runtime();
    assert!(working.block_on(service.update_time_setting(TimeSetting::TimeFormat, 3)).is_err());
    assert!(server.seen.lock().is_empty());
    working.block_on(service.update_time_setting(TimeSetting::DateFormat, 3)).unwrap();
    working.block_on(service.update_time_setting(TimeSetting::WeekStart, 7)).unwrap();
    assert_eq!(*server.seen.lock(), ["/core/v4/settings/dateformat", "/core/v4/settings/weekstart"]);
}

fn mail_folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("proton-service-{name}-{}", std::process::id()));
    crate::mail_store::erase(&dir);
    dir
}

#[test]
fn mail_comes_back_from_disk_after_a_restart_without_a_request() {
    let dir = mail_folder("restart");
    let working = runtime();
    let _inside = working.enter();
    let (first, _) = signed_in();
    first.use_mail_dir(Some(dir.clone()), None);
    first.store().unwrap().put("mail", "known", &*a_kept_message("known"));
    let key = first.store_key.lock().clone();

    let server = script(&[]);
    let (second, _) = mailbox_at(&server.base);
    *second.store_key.lock() = key;
    *second.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
    second.use_mail_dir(Some(dir.clone()), None);
    let stored = working.block_on(second.message("known")).unwrap();
    assert_eq!(stored.body, "text");
    assert!(server.seen.lock().is_empty());
    crate::mail_store::erase(&dir);
}

fn a_kept_attachment() -> crate::model::Attachment {
    crate::model::Attachment {
        id: String::new(),
        name: String::new(),
        size: 0,
        mime: String::new(),
        key_packets: String::new(),
        signed: false,
        inline: false,
        content_id: None,
        previewable: false,
    }
}

#[test]
fn a_picture_and_what_is_known_of_it_need_no_second_request() {
    let dir = mail_folder("pictures");
    let working = runtime();
    let _inside = working.enter();
    let server = script(&[]);
    let (service, _) = mailbox_at(&server.base);
    *service.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
    service.use_mail_dir(Some(dir.clone()), None);
    let store = service.store().unwrap();
    store.put("image", "https://a.example/b.png", &"data:image/png;base64,AA".to_string());
    store.put("tracker", "https://a.example/b.png", &Some("Tracker Ltd".to_string()));
    let picture = working.block_on(service.remote_image("https://a.example/b.png")).unwrap();
    let trackers = working.block_on(service.image_trackers(vec!["https://a.example/b.png".to_string()]));
    assert_eq!(picture, "data:image/png;base64,AA");
    assert_eq!(trackers.get("https://a.example/b.png").map(String::as_str), Some("Tracker Ltd"));
    assert!(server.seen.lock().is_empty());
    crate::mail_store::erase(&dir);
}

#[test]
fn files_inside_a_message_come_back_from_disk_with_it() {
    let dir = mail_folder("files");
    let working = runtime();
    let _inside = working.enter();
    let server = script(&[]);
    let (service, _) = mailbox_at(&server.base);
    *service.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
    service.use_mail_dir(Some(dir.clone()), None);
    let mut message = (*a_kept_message("packed")).clone();
    message.attachments.push(crate::model::Attachment { id: "mime-0".to_string(), ..a_kept_attachment() });
    let file = crate::mime::MimeAttachment { name: "a.pdf".into(), mime: "application/pdf".into(), content_id: None, inline: false, data: vec![1, 2, 3] };
    let store = service.store().unwrap();
    store.put("mail", "packed", &message);
    store.put("mime-files", "packed", &vec![file.clone()]);
    let opened = working.block_on(service.message("packed")).unwrap();
    assert_eq!(opened.attachments.len(), message.attachments.len());
    assert_eq!(service.mime_files.lock().iter().find(|(id, _)| id == "packed").map(|(_, files)| files[0].clone()), Some(file));
    assert!(server.seen.lock().is_empty());
    crate::mail_store::erase(&dir);
}

#[test]
fn a_stored_message_without_its_files_is_asked_for_again() {
    let dir = mail_folder("no-files");
    let working = runtime();
    let _inside = working.enter();
    let server = script(&[]);
    let (service, _) = mailbox_at(&server.base);
    *service.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
    service.use_mail_dir(Some(dir.clone()), None);
    let mut message = (*a_kept_message("packed")).clone();
    message.attachments.push(crate::model::Attachment { id: "mime-0".to_string(), ..a_kept_attachment() });
    service.store().unwrap().put("mail", "packed", &message);
    assert!(working.block_on(service.stored_message("packed")).is_none());
    crate::mail_store::erase(&dir);
}

#[test]
fn the_key_to_the_disk_is_saved_with_the_session() {
    let dir = mail_folder("key");
    let working = runtime();
    let _inside = working.enter();
    let (service, store) = signed_in();
    service.use_mail_dir(Some(dir.clone()), None);
    service.client.set_session(service.client.session());
    let saved: SavedSession = serde_json::from_str(&store.load().unwrap()).unwrap();
    assert!(saved.store_key.is_some());
    crate::mail_store::erase(&dir);
}

#[test]
fn a_lock_leaves_files_nobody_can_open_and_a_sign_out_leaves_none() {
    let dir = mail_folder("lock");
    let working = runtime();
    let _inside = working.enter();
    let (service, _) = signed_in();
    service.use_mail_dir(Some(dir.clone()), None);
    service.store().unwrap().put("mail", "known", &*a_kept_message("known"));
    service.wipe_memory();
    assert!(service.store().is_none());
    assert!(service.store_key.lock().is_none());
    assert!(dir.exists());
    service.forget_everything();
    assert!(!dir.exists());
}

#[test]
fn a_folder_opens_from_the_disk_before_proton_answers() {
    let dir = mail_folder("list");
    let working = runtime();
    let _inside = working.enter();
    let server = serve(|_, _| Some((200, a_page_of_mail(0))));
    let (service, updates) = mailbox_at(&server.base);
    *service.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
    service.use_mail_dir(Some(dir.clone()), None);
    let mut remembered = crate::view::View::default();
    let mark = remembered.open("archive");
    remembered.loaded(mark, vec![a_kept_message("old").meta.clone()]);
    remembered.confirm(100);
    service.store().unwrap().put("list", "archive", &remembered.as_stored().unwrap());
    working.block_on(service.view_open("archive"));
    let first = updates.lock().iter().find(|update| update.rows.is_some()).cloned().unwrap();
    assert_eq!(first.rows.as_ref().map(Vec::len), Some(1));
    assert_eq!(first.updated_at, Some(100));
    crate::mail_store::erase(&dir);
}

#[test]
fn a_logo_is_asked_for_by_domain_once_and_never_for_nonsense() {
    let server = serve(|path, _| Some(if path.contains("images/logo") { (404, "{}") } else { (200, r#"{"Code":1000,"MailSettings":{}}"#) }));
    let (service, _) = mailbox_at(&server.base);
    let working = runtime();
    assert_eq!(working.block_on(service.sender_logo("a@PayPal.de", false)), None);
    assert_eq!(working.block_on(service.sender_logo("b@paypal.de", false)), None);
    assert_eq!(working.block_on(service.sender_logo("not an address", false)), None);
    assert_eq!(working.block_on(service.sender_logo("x@evil/../path", false)), None);
    let logos: Vec<String> = server.seen.lock().iter().filter(|path| path.contains("images/logo")).cloned().collect();
    assert_eq!(logos.len(), 1);
    assert!(logos[0].contains("Domain=paypal.de") && !logos[0].contains("%40"));
}

#[test]
fn sender_images_turned_off_means_nothing_is_asked() {
    let server = serve(|_, _| Some((200, r#"{"Code":1000,"MailSettings":{"HideSenderImages":1}}"#)));
    let (service, _) = mailbox_at(&server.base);
    assert_eq!(runtime().block_on(service.sender_logo("a@paypal.de", false)), None);
    assert!(server.seen.lock().iter().all(|path| !path.contains("images/logo")));
}

/// Windows keeps a credential of 2560 bytes at most, and the keychain crate writes text there as
/// UTF-16. A session longer than that is refused, and the sign-in would not survive a restart.
#[test]
fn the_largest_session_fits_the_windows_credential_store() {
    let (service, store) = signed_in();
    let long = |length: usize| "x".repeat(length);
    // Proton's ids and tokens are 32 to 88 characters; these are longer than any seen.
    service.client.set_session(Some(Session {
        uid: long(128),
        access_token: long(128),
        refresh_token: long(128),
        password_mode: 2,
    }));
    // A passphrase from an old account without salts is the password itself.
    *service.key_secret.lock() = Some(Zeroizing::new(vec![b'p'; 128]));
    *service.store_key.lock() = Some(Zeroizing::new([7; 32]));
    runtime().block_on(service.set_app_password("app password")).unwrap();
    let stored = store.load().unwrap();
    assert!(stored.contains("sealed"), "the session was not sealed");
    assert!(stored.encode_utf16().count() * 2 <= crate::WINDOWS_CREDENTIAL_BYTES, "{} bytes", stored.len() * 2);
}

/// Gravatar takes the SHA-256 of the address, trimmed and in small letters, and is asked for a 404, not a made-up
/// picture, when it has none.
#[test]
fn a_gravatar_address_is_the_hash_of_the_address() {
    let expected = "https://www.gravatar.com/avatar/973dfe463ec85785f5f95af5ba3906eedb2d931c24e69824a89ea65dba4e813b?s=128&d=404";
    assert_eq!(super::images::gravatar_url("test@example.com").as_deref(), Some(expected));
    assert_eq!(super::images::gravatar_url("  Test@Example.COM ").as_deref(), Some(expected));
    for nothing in ["", "no-at-sign", "@example.com", "test@", "te st@example.com"] {
        assert_eq!(super::images::gravatar_url(nothing), None, "{nothing:?}");
    }
}
