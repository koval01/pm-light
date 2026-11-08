//! The mailbox shown a page at a time, against a local server that answers as Proton does.

use super::super::tests::{finishes, mailbox_at, rows_shown};
use crate::test_server::serve;

/// `count` messages from `first` on, and Proton's count of the whole folder.
fn counted_page(first: usize, count: usize, total: u64) -> &'static str {
    let rows = (first..first + count).map(|id| format!(r#"{{"ID":"{id}"}}"#)).collect::<Vec<_>>().join(",");
    format!(r#"{{"Code":1000,"Total":{total},"Messages":[{rows}]}}"#).leak()
}

fn page_asked(path: &str) -> Option<u64> {
    let query = path.strip_prefix("/mail/v4/messages?")?;
    query.split('&').find_map(|pair| pair.strip_prefix("Page="))?.parse().ok()
}

#[test]
fn a_page_is_asked_for_by_its_number_and_the_pages_are_counted() {
    let server = serve(|path, _| {
        Some((200, match page_asked(path) {
            Some(2) => counted_page(100, 20, 120),
            Some(page) => counted_page(usize::try_from(page).unwrap_or(0) * 50, 50, 120),
            None => r#"{"Code":1000}"#,
        }))
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
    let server = serve(|path, _| Some((200, if page_asked(path).is_some() { counted_page(0, 50, 120) } else { r#"{"Code":1000}"# })));
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
        Some((200, match page_asked(path) {
            Some(2) if total == 100 => counted_page(0, 0, 100),
            Some(2) => counted_page(100, 20, total),
            Some(page) => counted_page(usize::try_from(page).unwrap_or(0) * 50, 50, total),
            None => r#"{"Code":1000,"Responses":[]}"#,
        }))
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
