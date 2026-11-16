//! Every request to Proton while a check watches the app.
//!
//! What was asked how often, the most in any minute, the same request asked again within a second with no change
//! asked between, which a well-behaved client never does (a read after a change asks what the change made), and what
//! Proton answered with an error, which a client that asks for what is there seldom gets. Paths are
//! kept as `journal::clean_path` writes them, ids and names taken out, so that the summary can be shown anywhere; the
//! whole path stays in memory only, to tell a repeat from another request.

use crate::journal::clean_path;
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

/// Asked again this soon, with no change asked between, the same request is a repeat.
pub const REPEAT_WITHIN: Duration = Duration::from_secs(1);
const READ: &str = "GET";
const MINUTE: Duration = Duration::from_secs(60);
/// The first status that is an error, the client's or the server's.
const ERROR: u16 = 400;

struct Asked {
    at: Duration,
    method: String,
    path: String,
    refused: bool,
}

/// The requests of one run, from its start.
pub struct RequestLog {
    started: Instant,
    asked: Mutex<Vec<Asked>>,
    /// `STATUS METHOD /path/{id}` of each error Proton answered, with how often.
    turned_down: Mutex<BTreeMap<String, usize>>,
}

/// What a run asked of Proton, in numbers and paths without ids.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RequestSummary {
    pub total: usize,
    pub refused: usize,
    /// Each kind of request, `METHOD /path/{id}`, with how often it was asked.
    pub by_kind: Vec<(String, usize)>,
    pub most_in_a_minute: usize,
    /// Each kind asked again within `REPEAT_WITHIN`, with how often.
    pub repeats: Vec<(String, usize)>,
    /// Each request Proton answered with an error, `STATUS METHOD /path/{id}`, with how often.
    pub turned_down: Vec<(String, usize)>,
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestLog {
    #[must_use]
    pub fn new() -> Self {
        Self { started: Instant::now(), asked: Mutex::new(Vec::new()), turned_down: Mutex::new(BTreeMap::new()) }
    }

    /// Records a request. One asked again within `REPEAT_WITHIN` says so in the journal at once, so that what the app
    /// did just then shows beside it.
    pub fn record(&self, method: &str, path: &str, refused: bool) {
        if let Some(gap) = self.note(self.started.elapsed(), method, path, refused) {
            log::debug!("{method} {}: asked again after {} ms", clean_path(path), gap.as_millis());
        }
    }

    /// How long ago the same request was asked, when that was within `REPEAT_WITHIN`.
    fn note(&self, at: Duration, method: &str, path: &str, refused: bool) -> Option<Duration> {
        let mut asked = self.asked.lock();
        let read = method == READ;
        let gap = asked
            .iter()
            .rev()
            .take_while(|earlier| at.saturating_sub(earlier.at) < REPEAT_WITHIN)
            // A read after a change asks what the change made: the one before it is no answer to that.
            .take_while(|earlier| !read || earlier.method == READ)
            .find(|earlier| earlier.method == method && earlier.path == path)
            .map(|earlier| at.saturating_sub(earlier.at));
        asked.push(Asked { at, method: method.to_string(), path: path.to_string(), refused });
        drop(asked);
        gap
    }

    /// Records Proton's answer to a request: only an error is kept.
    pub fn answered(&self, method: &str, path: &str, status: u16) {
        if status >= ERROR {
            *self.turned_down.lock().entry(format!("{status} {method} {}", clean_path(path))).or_default() += 1;
        }
    }

    #[must_use]
    pub fn summary(&self) -> RequestSummary {
        let mut summary = summarize(&self.asked.lock());
        summary.turned_down = sorted(self.turned_down.lock().clone());
        summary
    }
}

fn kind(asked: &Asked) -> String {
    format!("{} {}", asked.method, clean_path(&asked.path))
}

fn summarize(asked: &[Asked]) -> RequestSummary {
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut repeats: BTreeMap<String, usize> = BTreeMap::new();
    let mut last_seen: HashMap<(&str, &str), Duration> = HashMap::new();
    let mut last_change: Option<Duration> = None;
    for request in asked {
        *by_kind.entry(kind(request)).or_default() += 1;
        let key = (request.method.as_str(), request.path.as_str());
        let read = request.method == READ;
        // A read after a change asks what the change made: the one before it is no answer to that.
        let repeated = last_seen.get(&key).is_some_and(|before| {
            request.at.saturating_sub(*before) < REPEAT_WITHIN && !(read && last_change.is_some_and(|at| at >= *before))
        });
        if repeated {
            *repeats.entry(kind(request)).or_default() += 1;
        }
        if !read {
            last_change = Some(request.at);
        }
        last_seen.insert(key, request.at);
    }
    // The busiest minute, by a window that slides from each request to the next sixty seconds.
    let mut most_in_a_minute = 0;
    let mut first = 0;
    for (last, request) in asked.iter().enumerate() {
        while asked.get(first).is_some_and(|earliest| request.at.saturating_sub(earliest.at) >= MINUTE) {
            first += 1;
        }
        most_in_a_minute = most_in_a_minute.max(last + 1 - first);
    }
    RequestSummary {
        total: asked.len(),
        refused: asked.iter().filter(|request| request.refused).count(),
        by_kind: sorted(by_kind),
        most_in_a_minute,
        repeats: sorted(repeats),
        turned_down: Vec::new(),
    }
}

/// The most often first, and in order of name among equals.
fn sorted(counts: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts
}

#[cfg(test)]
mod tests;
