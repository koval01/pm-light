//! What the mailbox screen shows: the open folder, its rows, what is selected and ticked. The page
//! draws this and reports what the person did. It decides nothing, as a UI thread would not.

use crate::model::{Counts, MessageChanges, MessageMeta};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub const PAGE: u64 = 50;
/// Proton refuses to move these out to these.
const CANNOT_LEAVE: [&str; 2] = ["sent", "drafts"];
const CANNOT_ENTER: [&str; 2] = ["inbox", "spam"];
/// Folders kept for a return visit, and how much of each: a long scroll is not worth its memory.
const SHELVES: usize = 16;
const SHELF_ROWS: usize = 150;
/// Hostile mail nobody asked to open, and drafts that may still change, wait for the click.
const NOT_AHEAD: [&str; 3] = ["spam", "trash", "drafts"];
const AHEAD_OF_A_FOLDER: usize = 5;
const AHEAD_OF_A_MESSAGE: usize = 2;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filters {
    pub keyword: String,
    pub unread: bool,
    pub has_attachments: bool,
}

#[must_use]
pub fn can_move(from: &str, target: &str) -> bool {
    !(CANNOT_LEAVE.contains(&from) && CANNOT_ENTER.contains(&target))
}

/// What the page needs after a change. Rows travel only when they changed: selecting a message is
/// a key press, and it should not cost the whole list.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewUpdate {
    pub folder: String,
    pub filters: Filters,
    pub selected: Option<String>,
    pub checked: Vec<String>,
    /// What an action without named messages applies to.
    pub targets: Vec<String>,
    pub has_more: bool,
    /// The page on screen, from 0, and how many there are. No pages means one list that grows as it is scrolled.
    pub page_at: u64,
    pub pages: u64,
    pub loading: bool,
    /// Loading what is on screen again, as opposed to fetching the next page.
    pub refreshing: bool,
    /// When Proton last confirmed this list, in seconds since the epoch.
    pub updated_at: Option<u64>,
    pub error: Option<Value>,
    pub refused_targets: Vec<&'static str>,
    /// The whole list, when it was replaced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<MessageMeta>>,
    /// The next page, on its own: sending the whole list again would cost more with every page.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<MessageMeta>,
    /// How many rows the list holds now, including any that did not travel.
    pub total: u64,
    /// Rows that were not there before this load: new mail, for the window to show as new.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub fresh: Vec<String>,
    /// Single rows that changed.
    pub changed: Vec<MessageMeta>,
    /// Rows that just left, for the page to animate away.
    pub removed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<Counts>,
}

/// A folder as it last looked, shown at once on a return while Proton is asked again.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StoredList {
    rows: Vec<MessageMeta>,
    has_more: bool,
    updated_at: Option<u64>,
    #[serde(default)]
    found: Option<u64>,
}

type Shelf = StoredList;

#[expect(clippy::struct_excessive_bools, reason = "the state of one screen, each a fact of its own")]
pub struct View {
    pub folder: String,
    filters: Filters,
    selected: Option<String>,
    checked: Vec<String>,
    rows: Vec<MessageMeta>,
    has_more: bool,
    loading: bool,
    more: bool,
    /// How many rows the last page really brought.
    added: usize,
    updated_at: Option<u64>,
    /// Oldest first.
    shelves: Vec<(String, Shelf)>,
    error: Option<Value>,
    /// Newer than any answer still on its way: those are dropped.
    navigation: u64,
    fresh: Vec<String>,
    scanned: Option<u64>,
    /// How many rows the account asks for at a time.
    page: u64,
    /// Pages, as Proton's own client shows them, instead of one list that grows as it is scrolled.
    paged: bool,
    /// The page on screen, from 0, when paged.
    page_at: u64,
    /// How many rows the folder holds with the filters on, as Proton counted them. Unknown for a search checked here,
    /// which reads more than it keeps.
    found: Option<u64>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            folder: "inbox".to_string(),
            filters: Filters::default(),
            selected: None,
            checked: Vec::new(),
            rows: Vec::new(),
            has_more: false,
            loading: false,
            more: false,
            added: 0,
            updated_at: None,
            shelves: Vec::new(),
            error: None,
            navigation: 0,
            fresh: Vec::new(),
            scanned: None,
            page: PAGE,
            paged: false,
            page_at: 0,
            found: None,
        }
    }
}

impl View {
    #[must_use]
    pub fn update(&self, whole: bool, changed: Vec<MessageMeta>, removed: Vec<String>) -> ViewUpdate {
        ViewUpdate {
            folder: self.folder.clone(),
            filters: self.filters.clone(),
            selected: self.selected.clone(),
            checked: self.checked.clone(),
            targets: self.targets(),
            has_more: self.has_more,
            page_at: self.page_at,
            pages: self.pages(),
            loading: self.loading,
            refreshing: self.loading && !self.more,
            updated_at: self.updated_at,
            error: self.error.clone(),
            refused_targets: CANNOT_ENTER.iter().copied().filter(|t| !can_move(&self.folder, t)).collect(),
            rows: whole.then(|| self.rows.clone()),
            added: Vec::new(),
            total: self.rows.len() as u64,
            fresh: self.fresh.clone(),
            changed,
            removed,
            counts: None,
        }
    }

    #[must_use]
    pub const fn filters(&self) -> &Filters {
        &self.filters
    }

    /// Where the next page starts. A search checked here has read more than it kept.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.scanned.unwrap_or(self.rows.len() as u64)
    }

    /// After a scan: how far it has read, and whether there is more to read.
    pub const fn scanned(&mut self, read: u64, more: bool) {
        self.scanned = Some(read);
        self.has_more = more;
    }

    #[must_use]
    pub fn row(&self, id: &str) -> Option<&MessageMeta> {
        self.rows.iter().find(|row| row.id == id)
    }

    /// Starts showing a folder from nothing. Returns the mark its answer must bring back.
    pub fn open(&mut self, folder: &str) -> u64 {
        self.shelve();
        self.folder = folder.to_string();
        self.filters = Filters::default();
        self.selected = None;
        self.checked.clear();
        self.page_at = 0;
        self.unshelve();
        self.begin()
    }

    /// Search results are not the folder, and a folder Proton never confirmed is not worth keeping.
    fn shelve(&mut self) {
        if self.filters != Filters::default() || self.updated_at.is_none() || self.page_at > 0 {
            return;
        }
        let shelf = Shelf {
            rows: self.rows.iter().take(SHELF_ROWS).cloned().collect(),
            has_more: self.has_more || self.rows.len() > SHELF_ROWS,
            updated_at: self.updated_at,
            found: self.found,
        };
        self.shelves.retain(|(kept, _)| *kept != self.folder);
        self.shelves.push((self.folder.clone(), shelf));
        if self.shelves.len() > SHELVES {
            self.shelves.remove(0);
        }
    }

    fn unshelve(&mut self) {
        let found = self.shelves.iter().position(|(kept, _)| *kept == self.folder);
        let shelf = found.map(|at| self.shelves.remove(at).1).unwrap_or_default();
        self.rows = shelf.rows;
        self.has_more = shelf.has_more;
        self.updated_at = shelf.updated_at;
        self.found = shelf.found;
        self.fit_to_page();
    }

    #[must_use]
    pub fn has_shelf(&self, folder: &str) -> bool {
        (self.folder == folder && self.updated_at.is_some()) || self.shelves.iter().any(|(kept, _)| kept == folder)
    }

    /// A folder as the disk remembers it, to show first after a start.
    pub fn stock(&mut self, folder: &str, list: StoredList) {
        self.shelves.retain(|(kept, _)| kept != folder);
        self.shelves.push((folder.to_string(), list));
    }

    /// What the screen shows, for the disk, unless it is a search.
    #[must_use]
    pub fn as_stored(&self) -> Option<StoredList> {
        (self.filters == Filters::default() && self.page_at == 0).then(|| StoredList {
            rows: self.rows.iter().take(SHELF_ROWS).cloned().collect(),
            has_more: self.has_more,
            updated_at: self.updated_at,
            found: self.found,
        })
    }

    /// Proton has just vouched for what is on screen.
    pub const fn confirm(&mut self, now: u64) {
        self.updated_at = Some(now);
    }

    /// What the person is likely to open next, to have it ready before they do.
    #[must_use]
    pub fn likely_next(&self, open: Option<&str>) -> Vec<String> {
        if NOT_AHEAD.contains(&self.folder.as_str()) || self.filters != Filters::default() {
            return Vec::new();
        }
        let (from, count) = open.map_or((0, AHEAD_OF_A_FOLDER), |id| {
                let at = self.rows.iter().position(|row| row.id == id);
                (at.map_or(self.rows.len(), |at| at + 1), AHEAD_OF_A_MESSAGE)
            });
        self.rows.iter().skip(from).take(count).map(|row| row.id.clone()).collect()
    }

    /// Starts loading again, keeping what is on screen.
    pub fn begin(&mut self) -> u64 {
        self.navigation += 1;
        self.error = None;
        self.loading = true;
        self.more = false;
        self.navigation
    }

    /// True when the folder's own rows came back, which the page has yet to be shown.
    pub fn set_filters(&mut self, filters: Filters) -> bool {
        let plain = Filters::default();
        if self.filters == plain && filters != plain {
            self.shelve();
        }
        let back = self.filters != plain && filters == plain;
        self.filters = filters;
        self.page_at = 0;
        if back {
            self.unshelve();
        }
        back
    }

    #[must_use]
    pub const fn page(&self) -> u64 {
        self.page
    }

    pub const fn set_page(&mut self, page: u64) {
        self.page = page;
        self.page_at = 0;
    }

    /// False if the person has already moved on, and this answer means nothing now.
    pub fn loaded(&mut self, mark: u64, rows: Vec<MessageMeta>) -> bool {
        if mark != self.navigation {
            return false;
        }
        self.scanned = None;
        let full = rows.len() as u64 >= self.page;
        // A search is its own list. A folder keeps what was scrolled to: a reload brings only the
        // first page, and a long list must not fall back to it.
        let old = std::mem::take(&mut self.rows);
        // Conversations and messages do not mix: what came before them goes.
        let same_kind = |old: &MessageMeta, new: &MessageMeta| (old.messages > 0) == (new.messages > 0);
        let alike = old.first().zip(rows.first()).is_none_or(|(old, new)| same_kind(old, new));
        // A later page has no new mail to show, and keeps nothing of the page before it.
        let plain = self.filters == Filters::default() && alike && self.page_at == 0;
        let newest = old.first().map(|row| row.date);
        let known: HashSet<&str> = old.iter().map(|row| row.id.as_str()).collect();
        self.fresh = rows
            .iter()
            .filter(|row| plain && newest.is_some_and(|date| row.date > date) && !known.contains(row.id.as_str()))
            .map(|row| row.id.clone())
            .collect();
        let page: HashSet<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        let oldest = rows.last().map_or(0, |row| row.date);
        let kept: Vec<MessageMeta> = old
            .iter()
            .filter(|row| plain && !self.paged && full && row.date <= oldest && !page.contains(row.id.as_str()))
            .cloned()
            .collect();
        self.has_more = if kept.is_empty() { full } else { self.has_more };
        self.rows = rows;
        self.rows.extend(kept);
        self.loading = false;
        self.more = false;
        true
    }

    pub fn failed(&mut self, mark: u64, error: Value) -> bool {
        if mark != self.navigation {
            return false;
        }
        self.error = Some(error);
        self.loading = false;
        self.more = false;
        true
    }

    /// A mark for the next page, unless there is none or a load is running.
    pub const fn begin_more(&mut self) -> Option<u64> {
        if self.loading || !self.has_more || self.pages() > 0 {
            return None;
        }
        self.loading = true;
        self.more = true;
        Some(self.navigation)
    }

    /// Pagination can repeat rows when new mail shifts the pages.
    pub fn appended(&mut self, mark: u64, page: Vec<MessageMeta>) -> bool {
        if mark != self.navigation {
            return false;
        }
        self.has_more = page.len() as u64 >= self.page;
        // One set for the page: looking each row up by itself grows with the whole list.
        let known: HashSet<&str> = self.rows.iter().map(|row| row.id.as_str()).collect();
        let new: Vec<MessageMeta> = page.into_iter().filter(|row| !known.contains(row.id.as_str())).collect();
        self.added += new.len();
        self.rows.extend(new);
        self.loading = false;
        self.more = false;
        true
    }

    /// What the last page added, and the state to go with it.
    pub fn update_with_added(&mut self) -> ViewUpdate {
        let start = self.rows.len().saturating_sub(self.added);
        let added = self.rows.get(start..).unwrap_or_default().to_vec();
        self.added = 0;
        ViewUpdate { added, ..self.update(false, Vec::new(), Vec::new()) }
    }

    /// The ticked messages, or else the open one.
    #[must_use]
    pub fn targets(&self) -> Vec<String> {
        if self.checked.is_empty() { self.selected.iter().cloned().collect() } else { self.checked.clone() }
    }

    pub fn select(&mut self, id: Option<String>) {
        self.selected = id;
    }

    #[must_use]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The row `delta` steps from the open one, kept within the list; from nothing, the matching end.
    #[must_use]
    pub fn relative(&self, delta: i64) -> Option<String> {
        let current = self.selected.as_ref().and_then(|id| self.rows.iter().position(|row| row.id == *id));
        let last = self.rows.len().checked_sub(1)?;
        let index = match current {
            None if delta > 0 => 0,
            None => last,
            Some(index) => {
                let moved = i64::try_from(index).ok()?.saturating_add(delta);
                usize::try_from(moved.max(0)).ok()?.min(last)
            }
        };
        self.rows.get(index).map(|row| row.id.clone())
    }

    pub fn toggle(&mut self, id: &str) {
        match self.checked.iter().position(|known| known == id) {
            Some(index) => drop(self.checked.remove(index)),
            None => self.checked.push(id.to_string()),
        }
    }

    pub fn check_all(&mut self) {
        self.checked = self.rows.iter().map(|row| row.id.clone()).collect();
    }

    pub fn clear_checks(&mut self) {
        self.checked.clear();
    }

    /// Takes rows out. If the open one goes, the next takes its place when the person wants that.
    pub fn take(&mut self, ids: &[String], next_on_move: bool) -> Vec<String> {
        if let Some(open) = self.selected.clone().filter(|open| ids.contains(open)) {
            let rest: Vec<&MessageMeta> =
                self.rows.iter().filter(|row| row.id == open || !ids.contains(&row.id)).collect();
            let at = rest.iter().position(|row| row.id == open);
            let next = at.and_then(|at| rest.get(at + 1).or_else(|| at.checked_sub(1).and_then(|i| rest.get(i))));
            self.selected = next.filter(|_| next_on_move).map(|row| row.id.clone());
        }
        self.checked.clear();
        self.rows.retain(|row| !ids.contains(&row.id));
        ids.to_vec()
    }

    pub fn clear(&mut self) {
        self.page_at = 0;
        self.rows.clear();
        self.selected = None;
        self.checked.clear();
    }

    fn edit(&mut self, ids: &[String], change: impl Fn(&mut MessageMeta)) -> Vec<MessageMeta> {
        let wanted = |row: &&mut MessageMeta| ids.contains(&row.id);
        self.rows
            .iter_mut()
            .filter(wanted)
            .map(|row| {
                change(row);
                row.clone()
            })
            .collect()
    }

    pub fn mark(&mut self, ids: &[String], changes: &MessageChanges) -> Vec<MessageMeta> {
        self.edit(ids, |row| {
            if let Some(unread) = changes.unread {
                row.unread = unread;
            }
            if let Some(starred) = changes.starred {
                row.starred = starred;
            }
        })
    }

    pub fn label(&mut self, ids: &[String], label: &str, on: bool) -> Vec<MessageMeta> {
        self.edit(ids, |row| {
            row.labels.retain(|known| known != label);
            if on {
                row.labels.push(label.to_string());
            }
        })
    }
}

mod pages;
#[cfg(test)]
mod tests;
