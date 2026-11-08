//! A folder shown a page at a time, as Proton's own client shows it, instead of one list that grows as it is
//! scrolled. Which of the two is the person's choice; the rows of a page are exactly what Proton sent for it.

use super::View;

impl View {
    /// Whether the folder is shown a page at a time. Changing it starts again from the first page.
    pub const fn set_paged(&mut self, paged: bool) {
        self.paged = paged;
        self.page_at = 0;
    }

    #[must_use]
    pub const fn is_paged(&self) -> bool {
        self.paged
    }

    /// How many pages there are, or 0 when the folder is one list: not paged, or not counted, as a search checked
    /// here is not. An empty folder is one empty page.
    #[must_use]
    pub const fn pages(&self) -> u64 {
        match self.found {
            Some(found) if self.paged && self.page > 0 => {
                let pages = found.div_ceil(self.page);
                if pages == 0 { 1 } else { pages }
            }
            _ => 0,
        }
    }

    /// Where the rows on screen start in the folder.
    #[must_use]
    pub const fn first_row(&self) -> u64 {
        if self.paged { self.page_at.saturating_mul(self.page) } else { 0 }
    }

    /// Goes to a page, kept within those there are. Returns the mark its answer must bring back, or nothing when the
    /// folder is not paged or the page is the one on screen.
    pub fn to_page(&mut self, page: u64) -> Option<u64> {
        let pages = self.pages();
        if pages == 0 {
            return None;
        }
        let page = page.min(pages - 1);
        if page == self.page_at && !self.loading {
            return None;
        }
        self.page_at = page;
        self.selected = None;
        self.checked.clear();
        Some(self.begin())
    }

    /// How many rows Proton says the folder holds with the filters on. `None` for a search checked here.
    pub const fn counted(&mut self, found: Option<u64>) {
        self.found = found;
    }

    /// After mail has gone from the last pages and left the one on screen empty: the last page there is, or else the
    /// one before, which is to be loaded. True if the view stepped back.
    pub const fn step_back_if_past(&mut self) -> bool {
        if !self.paged || self.page_at == 0 || !self.rows.is_empty() {
            return false;
        }
        let last = self.pages().saturating_sub(1);
        self.page_at = if last < self.page_at { last } else { self.page_at - 1 };
        true
    }

    /// A list kept from before shows only what fits on a page, so that the first page does not grow and shrink.
    pub(super) fn fit_to_page(&mut self) {
        if !self.paged {
            return;
        }
        let page = usize::try_from(self.page).unwrap_or(usize::MAX);
        if self.rows.len() > page {
            self.rows.truncate(page);
            self.has_more = true;
        }
    }
}

#[cfg(test)]
mod tests;
