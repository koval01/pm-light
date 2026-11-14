//! Every request to Proton while a check watches the app.
//!
//! What was asked how often, the most in any minute, and the same request asked again within a second, which a
//! well-behaved client never does. Paths are kept as `journal::clean_path` writes them, ids and names taken out, so
//! that the summary can be shown anywhere; the whole path stays in memory only, to tell a repeat from another request.

use crate::journal::clean_path;
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

/// Asked again this soon, the same request is a repeat.
pub const REPEAT_WITHIN: Duration = Duration::from_secs(1);
const MINUTE: Duration = Duration::from_secs(60);

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
}

impl Default for RequestLog {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestLog {
    #[must_use]
    pub fn new() -> Self {
        Self { started: Instant::now(), asked: Mutex::new(Vec::new()) }
    }

    pub fn record(&self, method: &str, path: &str, refused: bool) {
        let at = self.started.elapsed();
        self.asked.lock().push(Asked { at, method: method.to_string(), path: path.to_string(), refused });
    }

    #[must_use]
    pub fn summary(&self) -> RequestSummary {
        summarize(&self.asked.lock())
    }
}

fn kind(asked: &Asked) -> String {
    format!("{} {}", asked.method, clean_path(&asked.path))
}

fn summarize(asked: &[Asked]) -> RequestSummary {
    let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
    let mut repeats: BTreeMap<String, usize> = BTreeMap::new();
    let mut last_seen: HashMap<(&str, &str), Duration> = HashMap::new();
    for request in asked {
        *by_kind.entry(kind(request)).or_default() += 1;
        let key = (request.method.as_str(), request.path.as_str());
        if last_seen.get(&key).is_some_and(|before| request.at.saturating_sub(*before) < REPEAT_WITHIN) {
            *repeats.entry(kind(request)).or_default() += 1;
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
    let sorted = |counts: BTreeMap<String, usize>| {
        let mut counts: Vec<(String, usize)> = counts.into_iter().collect();
        counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        counts
    };
    RequestSummary {
        total: asked.len(),
        refused: asked.iter().filter(|request| request.refused).count(),
        by_kind: sorted(by_kind),
        most_in_a_minute,
        repeats: sorted(repeats),
    }
}

#[cfg(test)]
mod tests;
