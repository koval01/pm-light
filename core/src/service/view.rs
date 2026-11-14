//! What the mailbox screen can ask for. Each action changes `View`, tells the page at once, and then
//! talks to Proton: if that fails, the server is the truth and the list is read again.

use super::{Error, ListOptions, MessageChanges, MessageMeta, Ordering, Result, Service, Value, now_seconds};
use crate::{
    search,
    timing::{self, SnoozeChoice},
    view::{Filters, ViewUpdate, can_move},
};

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
        let view = self.view.lock();
        let filters = view.filters();
        let query = search::parse(&filters.keyword).query;
        let (begin, end) = search::begin_and_end(&query);
        ListOptions {
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
                self.store_list(&folder, &list);
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

    pub async fn view_open(&self, folder: &str) {
        // Asked of the disk at most once a folder: after that memory holds it.
        let known = self.view.lock().has_shelf(folder);
        if !known && let Some(list) = self.stored_list(folder).await {
            self.view.lock().stock(folder, list);
        }
        let mark = self.view.lock().open(folder);
        self.publish_rows();
        self.load(mark).await;
    }

    /// Whatever the screen last showed, for a page that has just loaded.
    pub async fn view_resume(&self) {
        let folder = self.view.lock().folder.clone();
        self.view_open(&folder).await;
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
    async fn refill(&self, result: Result<()>) -> Result<()> {
        let result = self.settle(result).await;
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

    /// What every change ends with. Counts and the list only move on the server's word.
    async fn settle(&self, result: Result<()>) -> Result<()> {
        self.threads.lock().clear();
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
        self.settle(result).await
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
        self.refill(result).await
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
        self.refill(result).await
    }

    pub async fn view_label(&self, ids: Option<Vec<String>>, label: &str, on: bool) -> Result<()> {
        let ids = self.targets(ids);
        let changed = self.view.lock().label(&ids, label, on);
        let update = self.view.lock().update(false, changed, Vec::new());
        self.publish(&update);
        let result = self.label_rows(&ids, label, on).await;
        if result.is_err() { self.settle(result).await } else { result }
    }

    pub async fn view_empty(&self) -> Result<()> {
        let folder = self.view.lock().folder.clone();
        let result = self.empty_folder(&folder).await;
        if result.is_ok() {
            self.view.lock().clear();
            self.publish_rows();
        }
        self.refill(result).await
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
        self.refill(result).await
    }

    pub async fn view_unsnooze(&self, id: &str) -> Result<()> {
        let Some(conversation) = self.conversation_of(id) else { return Ok(()) };
        self.remove_rows(&[id.to_string()]);
        let result = self.unsnooze(&conversation).await;
        self.refill(result).await
    }
}

#[cfg(test)]
mod tests;
