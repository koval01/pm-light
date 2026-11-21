//! Folders kept for a return visit: what a folder last looked like is shown at once while Proton is asked again, from
//! memory or, after a start, from the disk.

use super::{Filters, MessageMeta, View};
use serde::{Deserialize, Serialize};

/// Folders kept for a return visit, and how much of each: a long scroll is not worth its memory.
pub(super) const SHELVES: usize = 16;
const SHELF_ROWS: usize = 150;

/// A folder as it last looked, shown at once on a return while Proton is asked again.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StoredList {
    rows: Vec<MessageMeta>,
    has_more: bool,
    updated_at: Option<u64>,
    #[serde(default)]
    found: Option<u64>,
}

pub(super) type Shelf = StoredList;

impl View {
    /// Search results are not the folder, and a folder Proton never confirmed is not worth keeping.
    pub(super) fn shelve(&mut self) {
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

    pub(super) fn unshelve(&mut self) {
        let found = self.shelves.iter().position(|(kept, _)| *kept == self.folder);
        let shelf = found.map_or_default(|at| self.shelves.remove(at).1);
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
}
