//! What the mailbox screen can ask for. Each action changes `View`, tells the page at once, and then
//! talks to Proton: if that fails, the server is the truth and the list is read again.

use super::{Error, ListOptions, MessageChanges, MessageMeta, Ordering, Result, Service, Value, now_seconds};
use crate::{
    categories, search,
    timing::{self, SnoozeChoice},
    view::{Filters, ViewUpdate, can_move, place_of},
};

/// The folder that has categories.
const INBOX: &str = "inbox";

pub type ViewListener = Box<dyn Fn(&ViewUpdate) + Send + Sync>;

fn as_value(error: &Error) -> Value {
    serde_json::to_value(error).unwrap_or(Value::Null)
}

/// How much one step of a search checked here reads, at most.
const SCAN_AT_ONCE: u64 = 1000;

struct Scan {
    rows: Vec<MessageMeta>,
    read: u64,
    more: bool,
}

impl Service {
    pub fn on_view(&self, listener: ViewListener) {
        *self.view_listener.lock() = Some(listener);
    }

    fn publish(&self, update: &ViewUpdate) {
        if let Some(listener) = self.view_listener.lock().as_ref() {
            listener(update);
        }
    }

    fn publish_state(&self) {
        let update = self.view.lock().update(false, Vec::new(), Vec::new());
        self.publish(&update);
    }

    fn publish_rows(&self) {
        let update = self.view.lock().update(true, Vec::new(), Vec::new());
        self.publish(&update);
    }

    fn list_options(&self, offset: u64) -> ListOptions {
        // Apart: a guard lives to the end of its statement.
        let tab = self.view.lock().tab().map(str::to_string);
        let categories = tab.map_or_default(|tab| self.categories.read().labels_of(&tab));
        let view = self.view.lock();
        let filters = view.filters();
        let query = search::parse(&filters.keyword).query;
        let (begin, end) = search::begin_and_end(&query);
        ListOptions {
            categories,
            offset: Some(offset),
            limit: Some(view.page()),
            keyword: query.keyword_text(),
            unread: filters.unread.then_some(true).or(query.unread),
            has_attachments: (filters.has_attachments || query.has_attachment).then_some(true),
            from: query.from,
            to: query.to,
            subject: query.subject,
            begin,
            end,
            starred: query.starred,
        }
    }

    /// Reads pages until there is a page of matches or enough has been read for one step, because
    /// a pattern that matches nothing must not walk the whole mailbox by itself.
    async fn scan(&self, folder: &str, from: u64, query: &search::Query) -> Result<Scan> {
        let mut found = Vec::new();
        let mut read = from;
        loop {
            let page = self.list(folder, &self.list_options(read)).await?;
            let count = page.len() as u64;
            read += count;
            found.extend(page.into_iter().filter(|row| query.matches(row)));
            let page_size = self.view.lock().page();
            let more = count >= page_size;
            if !more || found.len() as u64 >= page_size || read - from >= SCAN_AT_ONCE {
                return Ok(Scan { rows: found, read, more });
            }
        }
    }

    fn query_checked_here(&self) -> Option<search::Query> {
        let query = search::parse(&self.view.lock().filters().keyword).query;
        query.checks_here().then_some(query)
    }

    async fn load(&self, mark: u64) {
        let mut mark = mark;
        // Mail that left the last pages can leave the one on screen empty: the last page there is is shown instead.
        for tries_left in (0..3).rev() {
            if !self.load_once(mark).await {
                return;
            }
            let mut view = self.view.lock();
            if tries_left == 0 || !view.step_back_if_past() {
                return;
            }
            mark = view.begin();
        }
    }

    /// True when the answer was for what is on screen.
    async fn load_once(&self, mark: u64) -> bool {
        // Apart: a guard lives to the end of its statement, and `list_options` takes the same lock.
        let folder = self.view.lock().folder.clone();
        let first = self.view.lock().first_row();
        let fresh = if let Some(query) = self.query_checked_here() {
            match self.scan(&folder, 0, &query).await {
                Ok(scan) => {
                    let mut view = self.view.lock();
                    let fresh = view.loaded(mark, scan.rows);
                    if fresh {
                        view.scanned(scan.read, scan.more);
                        view.counted(None);
                        view.confirm(now_seconds());
                    }
                    fresh
                }
                Err(error) => self.view.lock().failed(mark, as_value(&error)),
            }
        } else {
            let options = self.list_options(first);
            match self.list_counted(&folder, &options).await {
                Ok(listed) => {
                    let mut view = self.view.lock();
                    let fresh = view.loaded(mark, listed.rows);
                    if fresh {
                        view.counted(listed.total);
                        view.confirm(now_seconds());
                    }
                    fresh
                }
                Err(error) => self.view.lock().failed(mark, as_value(&error)),
            }
        };
        if fresh {
            self.publish_rows();
            let stored = self.view.lock().as_stored();
            if let Some(list) = stored {
                let place = self.view.lock().place();
                self.store_list(&place, &list);
            }
            let ahead = self.view.lock().likely_next(None);
            self.open_ahead(ahead);
        }
        fresh
    }

    /// The mail check found nothing new, so what is on screen is still the truth.
    pub fn view_confirmed(&self) {
        self.view.lock().confirm(now_seconds());
        self.publish_state();
    }

    /// A folder from its start. The inbox opens on Primary when the account shows it by category, as Proton's does.
    pub async fn view_open(&self, folder: &str) {
        let category = (folder == INBOX && self.categories.read().on()).then_some(categories::PRIMARY);
        self.view_open_in(folder, category).await;
    }

    /// One of the inbox's categories, from its start: Proton's tab drops filters, search and page alike.
    pub async fn view_category(&self, category: &str) {
        let shown = self.categories.read().has_tab(category);
        if shown {
            self.view_open_in(INBOX, Some(category)).await;
        }
    }

    async fn view_open_in(&self, folder: &str, category: Option<&str>) {
        // Asked of the disk at most once a place: after that memory holds it.
        let place = place_of(folder, category);
        let known = self.view.lock().has_shelf(&place);
        if !known && let Some(list) = self.stored_list(&place).await {
            self.view.lock().stock(&place, list);
        }
        let mark = self.view.lock().open_in(folder, category);
        self.publish_rows();
        self.load(mark).await;
    }

    /// Whatever the screen last showed, for a page that has just loaded. A category that went is Primary again.
    pub async fn view_resume(&self) {
        let (folder, tab) = {
            let view = self.view.lock();
            (view.folder.clone(), view.category().map(str::to_string))
        };
        let shown = tab.filter(|tab| self.categories.read().has_tab(tab));
        match shown {
            Some(tab) => self.view_category(&tab).await,
            None => self.view_open(&folder).await,
        }
    }

    pub async fn view_reload(&self) {
        let mark = self.view.lock().begin();
        self.publish_state();
        self.load(mark).await;
    }

    pub async fn view_filter(&self, filters: Filters) {
        let back = self.view.lock().set_filters(filters);
        if back {
            self.publish_rows();
        }
        self.view_reload().await;
    }

    /// Before the first folder opens: whether it is shown a page at a time.
    pub fn view_begin_paged(&self, paged: bool) {
        self.view.lock().set_paged(paged);
    }

    /// A page of the folder, from 0, when it is shown a page at a time.
    pub async fn view_page(&self, page: u64) {
        let Some(mark) = self.view.lock().to_page(page) else { return };
        self.publish_rows();
        self.load(mark).await;
    }

    /// The folder a page at a time, or as one list that grows as it is scrolled. Starts again from the top.
    pub async fn view_paged(&self, paged: bool) {
        if self.view.lock().is_paged() == paged {
            return;
        }
        self.view.lock().set_paged(paged);
        self.view_reload().await;
    }

    /// Mail has left a page: the next page's first rows move up into it.
    async fn refill(&self, touched: Option<&[String]>, result: Result<()>) -> Result<()> {
        let result = self.settle(touched, result).await;
        if result.is_ok() && self.view.lock().pages() > 0 {
            self.view_reload().await;
        }
        result
    }

    pub async fn view_more(&self) {
        let Some(mark) = self.view.lock().begin_more() else { return };
        self.publish_state();
        let (folder, offset) = {
            let view = self.view.lock();
            (view.folder.clone(), view.offset())
        };
        let fresh = if let Some(query) = self.query_checked_here() {
            match self.scan(&folder, offset, &query).await {
                Ok(scan) => {
                    let mut view = self.view.lock();
                    let fresh = view.appended(mark, scan.rows);
                    if fresh {
                        view.scanned(scan.read, scan.more);
                    }
                    fresh
                }
                Err(error) => self.view.lock().failed(mark, as_value(&error)),
            }
        } else {
            let options = self.list_options(offset);
            match self.list(&folder, &options).await {
                Ok(page) => self.view.lock().appended(mark, page),
                Err(error) => self.view.lock().failed(mark, as_value(&error)),
            }
        };
        if fresh {
            let update = self.view.lock().update_with_added();
            self.publish(&update);
        }
    }

    /// Opening an unread message reads it.
    pub async fn view_select(&self, id: Option<String>) -> Result<()> {
        self.view.lock().select(id.clone());
        self.publish_state();
        let ahead = self.view.lock().likely_next(id.as_deref());
        self.open_ahead(ahead);
        let unread = id.as_deref().is_some_and(|id| self.view.lock().row(id).is_some_and(|row| row.unread));
        match id.filter(|_| unread) {
            Some(id) => {
                self.view_mark(Some(vec![id]), MessageChanges { unread: Some(false), ..Default::default() }).await
            }
            None => Ok(()),
        }
    }

    pub async fn view_step(&self, delta: i64) -> Result<()> {
        let next = self.view.lock().relative(delta);
        if next.is_some() { self.view_select(next).await } else { Ok(()) }
    }

    pub fn view_toggle(&self, id: &str) {
        self.view.lock().toggle(id);
        self.publish_state();
    }

    pub fn view_check_all(&self, on: bool) {
        if on {
            self.view.lock().check_all();
        } else {
            self.view.lock().clear_checks();
        }
        self.publish_state();
    }

    fn targets(&self, ids: Option<Vec<String>>) -> Vec<String> {
        ids.unwrap_or_else(|| self.view.lock().targets())
    }

    /// What every change ends with. Counts and the list only move on the server's word. Only the conversations the
    /// change `touched` are read again: `None` is a change whose reach is not known.
    async fn settle(&self, touched: Option<&[String]>, result: Result<()>) -> Result<()> {
        self.forget_threads(touched);
        match &result {
            Ok(()) => match self.counts().await {
                Ok(counts) => {
                    let mut update = self.view.lock().update(false, Vec::new(), Vec::new());
                    update.counts = Some(counts);
                    self.publish(&update);
                }
                Err(error) => log::warn!("no new counts: {}", error.kind()),
            },
            Err(_) => self.view_reload().await,
        }
        result
    }

    fn remove_rows(&self, ids: &[String]) {
        let next = self.next_on_move.load(Ordering::Relaxed);
        let mut view = self.view.lock();
        let removed = view.take(ids, next);
        let update = view.update(true, Vec::new(), removed);
        drop(view);
        self.publish(&update);
    }

    pub async fn view_mark(&self, ids: Option<Vec<String>>, changes: MessageChanges) -> Result<()> {
        let ids = self.targets(ids);
        let changed = self.view.lock().mark(&ids, &changes);
        let update = self.view.lock().update(false, changed, Vec::new());
        self.publish(&update);
        let conversations = self.are_conversations(&ids);
        let result = self.change_rows(&ids, &changes, conversations).await;
        self.settle(Some(&ids), result).await
    }

    pub async fn view_move(&self, ids: Option<Vec<String>>, target: &str) -> Result<()> {
        let ids = self.targets(ids);
        if ids.is_empty() {
            return Ok(());
        }
        if !can_move(&self.view.lock().folder, target) {
            return Err(Error::CannotMove);
        }
        // Asked before the rows go: afterwards nothing tells a conversation from a message.
        let conversations = self.are_conversations(&ids);
        self.remove_rows(&ids);
        let changes = MessageChanges { folder: Some(target.to_string()), ..Default::default() };
        let result = self.change_rows(&ids, &changes, conversations).await;
        self.refill(Some(&ids), result).await
    }

    /// Mail moved to another of the inbox's categories, as Proton's client moves it (`useApplyLocation`): the whole
    /// conversation, whether the rows are conversations or messages. What leaves the tab on screen leaves the list, and
    /// the list is read again, since the rest of a conversation leaves with it.
    pub async fn view_categorize(&self, ids: Option<Vec<String>>, category: &str) -> Result<()> {
        if !self.categories.read().has_tab(category) {
            return Err(Error::CannotMove);
        }
        let ids = self.targets(ids);
        let tab = self.view.lock().tab().map(str::to_string);
        let Some(tab) = tab else { return Err(Error::CannotMove) };
        let stays = self.categories.read().labels_of(&tab).iter().any(|label| label == category);
        let mut conversations: Vec<String> = {
            let view = self.view.lock();
            ids.iter().filter_map(|id| view.row(id)).map(|row| row.conversation_id.clone()).collect()
        };
        conversations.sort();
        conversations.dedup();
        if conversations.is_empty() {
            return Ok(());
        }
        if !stays {
            self.remove_rows(&ids);
        }
        let body = serde_json::json!({"LabelID": category});
        let result =
            self.batch("/mail/v4/conversations/label", &conversations, "Could not move conversation", body).await;
        let result = self.settle(Some(&conversations), result).await;
        if result.is_ok() && !stays {
            self.view_reload().await;
        }
        result
    }

    pub async fn view_delete(&self, ids: Option<Vec<String>>) -> Result<()> {
        let ids = self.targets(ids);
        let folder = self.view.lock().folder.clone();
        let conversations = self.are_conversations(&ids);
        self.remove_rows(&ids);
        for id in &ids {
            self.forget_message(id);
        }
        let result = self.delete_rows(&ids, &folder, conversations).await;
        self.refill(Some(&ids), result).await
    }

    pub async fn view_label(&self, ids: Option<Vec<String>>, label: &str, on: bool) -> Result<()> {
        let ids = self.targets(ids);
        let changed = self.view.lock().label(&ids, label, on);
        let update = self.view.lock().update(false, changed, Vec::new());
        self.publish(&update);
        let result = self.label_rows(&ids, label, on).await;
        if result.is_err() { self.settle(Some(&ids), result).await } else { result }
    }

    pub async fn view_empty(&self) -> Result<()> {
        let folder = self.view.lock().folder.clone();
        let result = self.empty_folder(&folder).await;
        if result.is_ok() {
            self.view.lock().clear();
            self.publish_rows();
        }
        // Messages leave conversations no row names.
        self.refill(None, result).await
    }

    pub async fn view_not_spam(&self, id: &str) -> Result<()> {
        let message = self.message_of_row(id).await?;
        let marked = self.mark_not_spam(&message).await;
        let moved = self.view_move(Some(vec![id.to_string()]), "inbox").await;
        marked.and(moved)
    }

    fn conversation_of(&self, id: &str) -> Option<String> {
        self.view.lock().row(id).map(|row| row.conversation_id.clone())
    }

    pub async fn view_snooze(&self, id: &str, choice: SnoozeChoice) -> Result<()> {
        let Some(conversation) = self.conversation_of(id) else { return Ok(()) };
        let until = timing::snooze_time(choice)?;
        self.remove_rows(&[id.to_string()]);
        let result = self.snooze(&conversation, until).await;
        self.refill(Some(&[conversation]), result).await
    }

    pub async fn view_unsnooze(&self, id: &str) -> Result<()> {
        let Some(conversation) = self.conversation_of(id) else { return Ok(()) };
        self.remove_rows(&[id.to_string()]);
        let result = self.unsnooze(&conversation).await;
        self.refill(Some(&[conversation]), result).await
    }
}

#[cfg(test)]
mod tests;
