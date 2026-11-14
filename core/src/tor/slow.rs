//! Telling a slow circuit from a slow network, without leaving Tor.
//!
//! Each answer from Proton through Tor is timed to its headers. When the middle of the last few is slow, Quad9 is
//! asked a small question over its own circuit: if it answers quickly, the network is fine and the circuit to Proton
//! is the slow part, so new circuits are built. If Quad9 is slow too, the network is, and nothing changes. Nothing
//! here asks anything outside Tor: a check outside it would be the very leak Tor is chosen to prevent.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// How many answers are weighed together.
const WINDOW: usize = 5;
/// A middle answer slower than this, through Tor, to Proton's API, is a slow circuit or a slow network.
const SLOW: Duration = Duration::from_secs(5);
/// Answers right after new circuits are left out: the first one builds the circuit, which takes seconds by itself.
const SETTLING: u32 = 3;
/// Not asked again sooner, whatever the answers: new circuits each minute would only add relays to choose from.
const PAUSE: Duration = Duration::from_secs(120);
/// Quad9 over its own circuit within this is a network that works.
const QUICK: Duration = Duration::from_secs(3);

#[derive(Debug, Default)]
pub struct Watch {
    recent: VecDeque<Duration>,
    settling: u32,
    asked: Option<Instant>,
}

impl Watch {
    /// One answer from Proton took `took`. True when it is time to find out whether the circuit is the slow part.
    pub fn heard(&mut self, took: Duration, now: Instant) -> bool {
        if self.settling > 0 {
            self.settling -= 1;
            return false;
        }
        self.recent.push_back(took);
        while self.recent.len() > WINDOW {
            self.recent.pop_front();
        }
        if self.recent.len() < WINDOW || self.asked.is_some_and(|asked| now.saturating_duration_since(asked) < PAUSE) {
            return false;
        }
        if middle(&self.recent) <= SLOW {
            return false;
        }
        self.asked = Some(now);
        true
    }

    /// New circuits: what was heard over the old ones says nothing about them.
    pub fn renewed(&mut self) {
        self.recent.clear();
        self.settling = SETTLING;
    }

    /// The middle of the answers weighed now, for the journal.
    #[must_use]
    pub fn middle(&self) -> Duration {
        middle(&self.recent)
    }
}

fn middle(answers: &VecDeque<Duration>) -> Duration {
    let mut sorted: Vec<Duration> = answers.iter().copied().collect();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied().unwrap_or_default()
}

/// Whether Quad9, asked over its own circuit, answered like a network that works.
#[must_use]
pub fn network_works(quad9: Option<Duration>) -> bool {
    quad9.is_some_and(|took| took <= QUICK)
}

#[cfg(test)]
mod tests;
