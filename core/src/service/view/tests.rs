//! The mailbox as the view shows and changes it, against a local server that answers as Proton does.

use super::super::tests::{a_kept_message, finishes, mailbox_at, rows_shown};
use super::{MessageChanges, Service};
use crate::test_server::{Server, serve};

/// `count` messages from `first` on, and Proton's count of the whole folder.
fn counted_page(first: usize, count: usize, total: u64) -> String {
    let rows = (first..first + count).map(|id| format!(r#"{{"ID":"{id}"}}"#)).collect::<Vec<_>>().join(",");
    format!(r#"{{"Code":1000,"Total":{total},"Messages":[{rows}]}}"#)
}

fn page_asked(path: &str) -> Option<u64> {
    let query = path.strip_prefix("/mail/v4/messages?")?;
    query.split('&').find_map(|pair| pair.strip_prefix("Page="))?.parse().ok()
}

#[test]
fn a_page_is_asked_for_by_its_number_and_the_pages_are_counted() {
    let server = serve(|path, _| {
        Some((
            200,
            match page_asked(path) {
                Some(2) => counted_page(100, 20, 120),
                Some(page) => counted_page(usize::try_from(page).unwrap_or(0) * 50, 50, 120),
                None => r#"{"Code":1000}"#.to_string(),
            },
        ))
    });
    let (service, updates) = mailbox_at(&server.base);
    finishes(async move {
        service.view_paged(true).await;
        service.view_open("inbox").await;
        service.view_page(2).await;
    });
    let last = updates.lock().last().cloned().unwrap();
    assert_eq!((last.page_at, last.pages), (2, 3));
    assert_eq!(rows_shown(&updates), 20);
    let asked = server.seen.lock().iter().any(|path| path.contains("Page=2&PageSize=50"));
    assert!(asked, "{:?}", server.seen.lock());
}

#[test]
fn a_list_that_grows_shows_no_pages() {
    let server = serve(|path, _| {
        Some((200, if page_asked(path).is_some() { counted_page(0, 50, 120) } else { r#"{"Code":1000}"#.to_string() }))
    });
    let (service, updates) = mailbox_at(&server.base);
    finishes(async move {
        service.view_open("inbox").await;
        service.view_page(2).await;
    });
    let last = updates.lock().last().cloned().unwrap();
    assert_eq!((last.page_at, last.pages), (0, 0));
    assert!(!server.seen.lock().iter().any(|path| path.contains("Page=2")));
}

#[test]
fn deleting_the_last_page_away_shows_the_page_before() {
    // Until the delete, the folder holds 120; afterwards 100, and page 2 is empty.
    let deleted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen_delete = deleted.clone();
    let server = serve(move |path, _| {
        if path.contains("/delete") {
            seen_delete.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let total = if seen_delete.load(std::sync::atomic::Ordering::SeqCst) { 100 } else { 120 };
        Some((
            200,
            match page_asked(path) {
                Some(2) if total == 100 => counted_page(0, 0, 100),
                Some(2) => counted_page(100, 20, total),
                Some(page) => counted_page(usize::try_from(page).unwrap_or(0) * 50, 50, total),
                None => r#"{"Code":1000,"Responses":[]}"#.to_string(),
            },
        ))
    });
    let (service, updates) = mailbox_at(&server.base);
    finishes(async move {
        service.view_paged(true).await;
        service.view_open("trash").await;
        service.view_page(2).await;
        service.view_check_all(true);
        let _ = service.view_delete(None).await;
    });
    assert!(deleted.load(std::sync::atomic::Ordering::SeqCst), "{:?}", server.seen.lock());
    let last = updates.lock().last().cloned().unwrap();
    assert_eq!((last.page_at, last.pages), (1, 2));
    assert_eq!(rows_shown(&updates), 50);
    assert!(!last.loading);
}

/// Proton for two conversations, `c0` and `c1`, each holding one unread message, `m0` and `m1`. It takes every change.
fn two_conversations(path: &str) -> (u16, String) {
    let thread = |message: &str| {
        format!(r#"{{"Code":1000,"Messages":[{{"ID":"{message}","LabelIDs":["0"],"Time":1,"Unread":1}}]}}"#)
    };
    match path.split('?').next().unwrap_or(path) {
        "/mail/v4/conversations/c0" => (200, thread("m0")),
        "/mail/v4/conversations/c1" => (200, thread("m1")),
        "/mail/v4/conversations/count" | "/mail/v4/messages/count" => (200, r#"{"Code":1000,"Counts":[]}"#.to_string()),
        _ => (200, r#"{"Code":1000,"Responses":[]}"#.to_string()),
    }
}

/// `folder` showing `ids`, unread, as conversations when `conversations` and as single messages otherwise.
fn shows(service: &Service, folder: &str, ids: &[&str], conversations: bool) {
    let rows = ids
        .iter()
        .map(|id| {
            let mut row = a_kept_message(id).meta.clone();
            row.messages = u32::from(conversations);
            row
        })
        .collect();
    let mut view = service.view.lock();
    let mark = view.open(folder);
    let loaded = view.loaded(mark, rows);
    drop(view);
    assert!(loaded);
}

fn asked(server: &Server, path: &str) -> usize {
    server.seen.lock().iter().filter(|seen| seen.split('?').next() == Some(path)).count()
}

/// Both conversations asked for, as a read ahead asks: a second time only when they were forgotten.
async fn read_both(service: &Service) {
    let _ = service.thread("c0").await;
    let _ = service.thread("c1").await;
}

fn read() -> MessageChanges {
    MessageChanges { unread: Some(false), ..MessageChanges::default() }
}

/// Opening an unread conversation reads it, and that forgot every conversation read ahead: in an inbox of unread mail
/// each mail opened asked for them all again, and the long run made more than twice the requests.
#[test]
fn reading_one_conversation_keeps_the_others() {
    let server = serve(|path, _| Some(two_conversations(path)));
    let (service, _) = mailbox_at(&server.base);
    shows(&service, "inbox", &["c0", "c1"], true);
    finishes(async move {
        read_both(&service).await;
        assert!(service.view_mark(Some(vec!["c0".into()]), read()).await.is_ok());
        read_both(&service).await;
    });
    assert_eq!(asked(&server, "/mail/v4/conversations/read"), 1, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c1"), 1, "{:?}", server.seen.lock());
    // The one read is asked again: which of its messages opens depends on what is unread.
    assert_eq!(asked(&server, "/mail/v4/conversations/c0"), 2, "{:?}", server.seen.lock());
}

/// The account check refuses every change, so an unread conversation stays unread and is marked again at each visit.
#[test]
fn a_refused_change_keeps_the_conversations_it_did_not_touch() {
    let server = serve(|path, _| Some(two_conversations(path)));
    let (service, _) = mailbox_at(&server.base);
    service.client.forbid_changes();
    shows(&service, "inbox", &["c0", "c1"], true);
    finishes(async move {
        read_both(&service).await;
        assert!(service.view_mark(Some(vec!["c0".into()]), read()).await.is_err());
        read_both(&service).await;
    });
    assert_eq!(asked(&server, "/mail/v4/conversations/read"), 0, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c1"), 1, "{:?}", server.seen.lock());
}

/// A message shown on its own belongs to a conversation all the same: the one that holds it is asked again.
#[test]
fn a_changed_message_takes_only_its_own_conversation_along() {
    let server = serve(|path, _| Some(two_conversations(path)));
    let (service, _) = mailbox_at(&server.base);
    shows(&service, "inbox", &["m0", "m1"], false);
    finishes(async move {
        read_both(&service).await;
        assert!(service.view_mark(Some(vec!["m1".into()]), read()).await.is_ok());
        read_both(&service).await;
    });
    assert_eq!(asked(&server, "/mail/v4/messages/read"), 1, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c0"), 1, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c1"), 2, "{:?}", server.seen.lock());
}

#[test]
fn a_moved_conversation_takes_only_itself_along() {
    let server = serve(|path, _| Some(two_conversations(path)));
    let (service, _) = mailbox_at(&server.base);
    shows(&service, "inbox", &["c0", "c1"], true);
    finishes(async move {
        read_both(&service).await;
        assert!(service.view_move(Some(vec!["c1".into()]), "archive").await.is_ok());
        read_both(&service).await;
    });
    assert_eq!(asked(&server, "/mail/v4/conversations/label"), 1, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c0"), 1, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c1"), 2, "{:?}", server.seen.lock());
}

/// Emptying a folder takes messages out of conversations no row names: every conversation is asked again.
#[test]
fn emptying_a_folder_forgets_every_conversation() {
    let server = serve(|path, _| Some(two_conversations(path)));
    let (service, _) = mailbox_at(&server.base);
    shows(&service, "trash", &["c0"], true);
    finishes(async move {
        read_both(&service).await;
        assert!(service.view_empty().await.is_ok());
        read_both(&service).await;
    });
    assert_eq!(asked(&server, "/mail/v4/messages/empty"), 1, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c0"), 2, "{:?}", server.seen.lock());
    assert_eq!(asked(&server, "/mail/v4/conversations/c1"), 2, "{:?}", server.seen.lock());
}
