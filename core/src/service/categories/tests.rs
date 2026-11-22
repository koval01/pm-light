//! The inbox by category, against a local server that answers as Proton does: what is asked for, and what is shown.

use super::super::tests::{finishes, mail_folder, mailbox_at, rows_shown};
use crate::{
    test_server::{Server, serve},
    view::{Filters, View, ViewUpdate},
};
use parking_lot::Mutex;
use std::sync::Arc;
use zeroize::Zeroizing;

/// How an account stands: whether it pays, whether it chose categories, which are shown, how its organization
/// stands, and whether mail is grouped in conversations.
#[derive(Clone, Copy)]
#[expect(clippy::struct_excessive_bools, reason = "an account in a test, each a fact of its own")]
struct Account {
    subscribed: bool,
    chosen: bool,
    shown: &'static [&'static str],
    organization_allows: bool,
    labels_answer: u16,
    conversations: bool,
}

const FREE: Account = Account {
    subscribed: false,
    chosen: true,
    shown: &["24", "20", "21", "25"],
    organization_allows: false,
    labels_answer: 200,
    conversations: false,
};

fn labels(shown: &[&str]) -> String {
    let rows: Vec<String> = ["24", "20", "21", "25", "26", "22", "1", "5"]
        .iter()
        .map(|id| format!(r#"{{"ID":"{id}","Name":"n","Type":4,"Display":{}}}"#, u8::from(shown.contains(id))))
        .collect();
    format!(r#"{{"Code":1000,"Labels":[{}]}}"#, rows.join(","))
}

/// Mail whose ids say which list it came from, so that what is shown tells what was asked.
fn listed(path: &str) -> String {
    let tab = path.split(['?', '&']).filter_map(|pair| pair.strip_prefix("LabelID%5B%5D=")).nth(1).unwrap_or("whole");
    let rows: Vec<String> = (0..3)
        .map(|n| format!(r#"{{"ID":"{tab}-{n}","ConversationID":"c{tab}-{n}","Time":{},"LabelIDs":["0"]}}"#, 100 - n))
        .collect();
    if path.starts_with("/mail/v4/conversations?") {
        let conversations: Vec<String> = (0..3)
            .map(|n| format!(r#"{{"ID":"{tab}-{n}","Time":{},"NumMessages":2,"Labels":[{{"ID":"0"}}]}}"#, 100 - n))
            .collect();
        return format!(r#"{{"Code":1000,"Total":3,"Conversations":[{}]}}"#, conversations.join(","));
    }
    format!(r#"{{"Code":1000,"Total":3,"Messages":[{}]}}"#, rows.join(","))
}

fn proton(account: Account) -> Server {
    serve(move |path, _| {
        let answer = match path.split('?').next().unwrap_or_default() {
            "/core/v4/users" => {
                (200, format!(r#"{{"Code":1000,"User":{{"Subscribed":{}}}}}"#, u8::from(account.subscribed)))
            }
            "/mail/v4/settings" => (
                200,
                format!(
                    r#"{{"Code":1000,"MailSettings":{{"MailCategoryView":{},"ViewMode":{}}}}}"#,
                    account.chosen,
                    u8::from(!account.conversations)
                ),
            ),
            "/core/v4/organizations/settings" => {
                (200, format!(r#"{{"Code":1000,"MailCategoryViewEnabled":{}}}"#, account.organization_allows))
            }
            "/core/v4/labels" => (account.labels_answer, labels(account.shown)),
            "/mail/v4/messages" | "/mail/v4/conversations" => (200, listed(path)),
            _ => (200, r#"{"Code":1000,"Counts":[]}"#.to_string()),
        };
        Some(answer)
    })
}

type Updates = Arc<Mutex<Vec<ViewUpdate>>>;

fn opened(account: Account) -> (Server, Arc<crate::service::Service>, Updates) {
    let server = proton(account);
    let (service, updates) = mailbox_at(&server.base);
    let reading = service.clone();
    finishes(async move {
        futures::join!(reading.read_habits(), reading.read_categories());
        reading.view_open("inbox").await;
    });
    (server, service, updates)
}

fn lists_asked(server: &Server) -> Vec<String> {
    let seen = server.seen.lock();
    seen.iter()
        .filter(|path| path.starts_with("/mail/v4/messages?") || path.starts_with("/mail/v4/conversations?"))
        .cloned()
        .collect()
}

fn last(updates: &Updates) -> ViewUpdate {
    updates.lock().last().cloned().unwrap()
}

fn shown_ids(updates: &Updates) -> Vec<String> {
    let rows = updates.lock().iter().rev().find_map(|update| update.rows.clone()).unwrap_or_default();
    rows.into_iter().map(|row| row.id).collect()
}

#[test]
fn the_inbox_opens_on_primary_with_the_hidden_categories_in_it() {
    let (server, _, updates) = opened(FREE);
    let asked = lists_asked(&server);
    assert_eq!(asked.len(), 1, "{asked:?}");
    let query = asked.first().unwrap();
    assert!(
        query.starts_with(
            "/mail/v4/messages?LabelID%5B%5D=0&LabelID%5B%5D=24&LabelID%5B%5D=26&LabelID%5B%5D=22&LabelID%5B%5D=23&Page=0"
        ),
        "{query}"
    );
    assert!(!query.contains("LabelID=0"), "the folder alone is not asked as well: {query}");
    assert_eq!(last(&updates).category.as_deref(), Some("24"));
    assert_eq!(shown_ids(&updates), ["24-0", "24-1", "24-2"]);
}

#[test]
fn a_free_account_is_not_asked_about_an_organization() {
    let (server, ..) = opened(FREE);
    assert!(!server.seen.lock().iter().any(|path| path.contains("organizations")));
}

#[test]
fn a_tab_shows_its_own_mail_and_nothing_else() {
    let (server, service, updates) = opened(FREE);
    finishes(async move { service.view_category("25").await });
    let asked = lists_asked(&server);
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked.last().unwrap().starts_with("/mail/v4/messages?LabelID%5B%5D=0&LabelID%5B%5D=25&Page=0"));
    assert_eq!(last(&updates).category.as_deref(), Some("25"));
    assert_eq!(last(&updates).folder, "inbox");
    assert_eq!(shown_ids(&updates), ["25-0", "25-1", "25-2"]);
}

#[test]
fn a_hidden_or_made_up_category_opens_nothing() {
    let (server, service, updates) = opened(FREE);
    finishes(async move {
        for wrong in ["26", "22", "23", "0", "", "24&LabelID=5"] {
            service.view_category(wrong).await;
        }
    });
    assert_eq!(lists_asked(&server).len(), 1);
    assert_eq!(last(&updates).category.as_deref(), Some("24"));
}

#[test]
fn conversations_are_asked_for_by_category_as_well() {
    let (server, service, updates) = opened(Account { conversations: true, ..FREE });
    finishes(async move { service.view_category("20").await });
    let asked = lists_asked(&server);
    assert!(asked.last().unwrap().starts_with("/mail/v4/conversations?LabelID%5B%5D=0&LabelID%5B%5D=20&"), "{asked:?}");
    let rows = updates.lock().iter().rev().find_map(|update| update.rows.clone()).unwrap();
    assert!(rows.iter().all(|row| row.messages == 2 && row.folder == "inbox"), "rows stay the inbox's conversations");
}

#[test]
fn a_search_covers_the_whole_inbox_and_hides_the_tabs_until_it_is_cleared() {
    let (server, service, updates) = opened(FREE);
    let searching = service.clone();
    finishes(async move {
        searching.view_category("21").await;
        searching.view_filter(Filters { keyword: "invoice".into(), ..Filters::default() }).await;
    });
    let asked = lists_asked(&server);
    let search = asked.last().unwrap();
    assert!(search.starts_with("/mail/v4/messages?LabelID=0&") && search.contains("Keyword=invoice"), "{search}");
    assert_eq!(last(&updates).category, None);
    finishes(async move { service.view_filter(Filters::default()).await });
    assert_eq!(last(&updates).category.as_deref(), Some("21"));
    assert!(lists_asked(&server).last().unwrap().contains("LabelID%5B%5D=21"));
}

#[test]
fn filters_and_pages_stay_inside_the_tab() {
    let (server, service, _) = opened(FREE);
    finishes(async move {
        service.view_category("20").await;
        service.view_filter(Filters { unread: true, has_attachments: true, ..Filters::default() }).await;
        service.view_more().await;
    });
    let asked = lists_asked(&server);
    let filtered = asked.get(2).unwrap();
    assert!(filtered.contains("LabelID%5B%5D=0&LabelID%5B%5D=20") && filtered.contains("Unread=1"), "{filtered}");
    assert!(filtered.contains("Attachments=1"), "{filtered}");
}

#[test]
fn a_tab_shown_a_page_at_a_time_is_asked_and_counted_with_its_category() {
    let (server, service, updates) = opened(FREE);
    finishes(async move {
        service.view_paged(true).await;
        service.view_category("21").await;
    });
    let shown = last(&updates);
    assert_eq!((shown.category.as_deref(), shown.pages), (Some("21"), 1));
    let asked = lists_asked(&server);
    assert!(asked.iter().all(|path| !path.contains("LabelID=0")), "{asked:?}");
    assert!(asked.last().unwrap().contains("LabelID%5B%5D=0&LabelID%5B%5D=21&Page=0"), "{asked:?}");
}

#[test]
fn going_back_to_the_inbox_opens_primary_again() {
    let (server, service, updates) = opened(FREE);
    finishes(async move {
        service.view_category("25").await;
        service.view_open("archive").await;
        service.view_open("inbox").await;
    });
    assert!(lists_asked(&server).iter().any(|path| path.starts_with("/mail/v4/messages?LabelID=6&")));
    assert_eq!(last(&updates).category.as_deref(), Some("24"));
}

#[test]
fn another_folder_has_no_tabs() {
    let (_, service, updates) = opened(FREE);
    finishes(async move { service.view_open("archive").await });
    assert_eq!(last(&updates).category, None);
}

#[test]
fn a_resumed_page_comes_back_to_the_tab_it_showed() {
    let (server, service, updates) = opened(FREE);
    finishes(async move {
        service.view_category("20").await;
        service.view_resume().await;
    });
    assert_eq!(last(&updates).category.as_deref(), Some("20"));
    assert!(lists_asked(&server).last().unwrap().contains("LabelID%5B%5D=20"));
}

#[test]
fn a_tab_seen_before_shows_its_own_rows_at_once_on_a_return() {
    let (_, service, updates) = opened(FREE);
    let switching = service.clone();
    finishes(async move {
        switching.view_category("20").await;
        switching.view_category("24").await;
    });
    // The first rows shown on the way back are Primary's own, kept from before, not Social's.
    let returned = updates.lock().iter().rev().filter_map(|update| update.rows.clone()).nth(1).unwrap();
    assert_eq!(returned.first().map(|row| row.id.as_str()), Some("24-0"));
    drop(service);
}

#[test]
fn a_tab_opens_from_the_disk_before_proton_answers() {
    let dir = mail_folder("category-list");
    let server = proton(FREE);
    let (working, updates) = mailbox_at(&server.base);
    finishes(async move {
        *working.key_secret.lock() = Some(Zeroizing::new(b"key secret".to_vec()));
        working.use_mail_dir(Some(dir.clone()), None);
        let mut remembered = View::default();
        let mark = remembered.open_in("inbox", Some("20"));
        let mut row = super::super::tests::a_kept_message("old").meta.clone();
        row.id = "kept".into();
        remembered.loaded(mark, vec![row]);
        remembered.confirm(100);
        working.store().unwrap().put("list", "inbox#20", &remembered.as_stored().unwrap());
        futures::join!(working.read_habits(), working.read_categories());
        working.view_category("20").await;
        crate::mail_store::erase(&dir);
    });
    let first = updates.lock().iter().find(|update| update.rows.is_some()).cloned().unwrap();
    assert_eq!(first.rows.unwrap().first().map(|row| row.id.clone()), Some("kept".into()));
    assert_eq!(first.category.as_deref(), Some("20"));
    assert_eq!(rows_shown(&updates), 3);
}

#[test]
fn an_account_that_turned_categories_off_has_one_inbox() {
    let (server, _, updates) = opened(Account { chosen: false, ..FREE });
    assert!(lists_asked(&server).first().unwrap().starts_with("/mail/v4/messages?LabelID=0&"));
    assert_eq!(last(&updates).category, None);
}

#[test]
fn without_the_categories_answer_the_inbox_stays_whole() {
    for status in [404, 422, 500] {
        let (server, _, updates) = opened(Account { labels_answer: status, ..FREE });
        assert!(lists_asked(&server).first().unwrap().starts_with("/mail/v4/messages?LabelID=0&"), "{status}");
        assert_eq!(last(&updates).category, None);
    }
}

#[test]
fn a_paying_account_follows_its_organization() {
    let (server, _, updates) = opened(Account { subscribed: true, organization_allows: false, ..FREE });
    assert!(server.seen.lock().iter().any(|path| path == "/core/v4/organizations/settings"));
    assert_eq!(last(&updates).category, None);
    let (_, _, updates) = opened(Account { subscribed: true, organization_allows: true, ..FREE });
    assert_eq!(last(&updates).category.as_deref(), Some("24"));
}

#[test]
fn counts_are_asked_for_the_inbox_alone_where_categories_are_on() {
    for (account, expected) in [
        (FREE, Some("OnlyInInboxForCategories=1")),
        (Account { chosen: false, ..FREE }, Some("OnlyInInboxForCategories=0")),
        (Account { subscribed: true, organization_allows: false, ..FREE }, None),
    ] {
        let (server, service, _) = opened(account);
        assert!(finishes(async move { service.counts().await }).is_ok());
        let seen = server.seen.lock().clone();
        let asked = seen.iter().find(|path| path.starts_with("/mail/v4/messages/count")).unwrap();
        match expected {
            Some(parameter) => assert_eq!(*asked, format!("/mail/v4/messages/count?{parameter}")),
            None => assert_eq!(asked, "/mail/v4/messages/count"),
        }
    }
}

fn asked(server: &Server, path: &str) -> bool {
    server.seen.lock().iter().any(|seen| seen == path)
}

#[test]
fn mail_moved_to_another_tab_moves_its_whole_conversation_and_leaves_the_list() {
    let (server, moving, updates) = opened(FREE);
    let result = finishes(async move {
        moving.view_category("20").await;
        moving.view_categorize(Some(vec!["20-1".into()]), "25").await
    });
    assert!(result.is_ok(), "{result:?}");
    assert!(asked(&server, "/mail/v4/conversations/label"));
    assert!(!asked(&server, "/mail/v4/messages/label"), "a message's id never goes as the move's");
    assert!(updates.lock().iter().any(|update| update.removed.iter().any(|id| id == "20-1")));
    assert!(lists_asked(&server).last().unwrap().contains("LabelID%5B%5D=20"), "the tab is read again");
}

#[test]
fn mail_moved_to_the_tab_that_holds_it_stays() {
    let (server, service, updates) = opened(FREE);
    let result = finishes(async move { service.view_categorize(Some(vec!["24-0".into()]), "24").await });
    assert!(result.is_ok());
    assert!(asked(&server, "/mail/v4/conversations/label"));
    assert!(updates.lock().iter().all(|update| update.removed.is_empty()));
}

#[test]
fn mail_goes_only_to_a_category_that_has_a_tab() {
    let (server, service, _) = opened(FREE);
    let results = finishes(async move {
        let mut results = Vec::new();
        for wrong in ["26", "22", "23", "0", "6", "", "../labels"] {
            results.push(service.view_categorize(Some(vec!["24-0".into()]), wrong).await);
        }
        results
    });
    assert!(results.iter().all(|result| matches!(result, Err(crate::error::Error::CannotMove))), "{results:?}");
    assert!(!asked(&server, "/mail/v4/conversations/label"));
}

#[test]
fn nothing_moves_to_a_category_outside_the_inbox_or_in_a_search() {
    let (server, service, _) = opened(FREE);
    let results = finishes(async move {
        service.view_filter(Filters { keyword: "sale".into(), ..Filters::default() }).await;
        let searching = service.view_categorize(Some(vec!["whole-0".into()]), "20").await;
        service.view_open("archive").await;
        let elsewhere = service.view_categorize(Some(vec!["whole-0".into()]), "20").await;
        [searching, elsewhere]
    });
    assert!(results.iter().all(|result| matches!(result, Err(crate::error::Error::CannotMove))));
    assert!(!asked(&server, "/mail/v4/conversations/label"));
}

#[test]
fn categories_are_turned_off_and_the_inbox_is_one_list_again() {
    let (server, service, updates) = opened(FREE);
    let result = finishes(async move {
        let result = service.set_categories_on(false).await;
        service.view_open("inbox").await;
        result
    });
    assert!(result.is_ok());
    assert!(asked(&server, "/mail/v4/settings/mail-category-view"));
    assert_eq!(last(&updates).category, None);
    assert!(lists_asked(&server).last().unwrap().starts_with("/mail/v4/messages?LabelID=0&"));
}

#[test]
fn a_tab_is_hidden_and_its_mail_goes_to_primary() {
    let (server, service, updates) = opened(FREE);
    let result = finishes(async move {
        let result = service.show_category("21", false).await;
        service.view_open("inbox").await;
        result
    });
    assert!(result.is_ok());
    assert!(asked(&server, "/core/v4/labels/21"));
    assert_eq!(last(&updates).category.as_deref(), Some("24"));
    let primary = lists_asked(&server).last().unwrap().clone();
    assert!(primary.contains("LabelID%5B%5D=21"), "Primary now holds Promotions: {primary}");
}

#[test]
fn primary_and_what_is_no_category_cannot_be_hidden() {
    let (server, service, _) = opened(FREE);
    let results = finishes(async move {
        let mut results = Vec::new();
        for wrong in ["24", "0", "23", "../users", ""] {
            results.push(service.show_category(wrong, false).await);
        }
        results
    });
    assert!(results.iter().all(Result::is_err));
    assert!(!server.seen.lock().iter().any(|path| path.starts_with("/core/v4/labels/")));
}
