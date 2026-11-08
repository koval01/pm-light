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
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn quick_answers_never_ask() {
        let (mut watch, now) = (Watch::default(), Instant::now());
        assert!((0..50).all(|_| !watch.heard(MS * 600, now)));
    }

    #[test]
    fn slow_answers_ask_once_and_then_rest() {
        let (mut watch, now) = (Watch::default(), Instant::now());
        let asked: Vec<bool> = (0..WINDOW).map(|_| watch.heard(MS * 9000, now)).collect();
        assert_eq!(asked, [false, false, false, false, true]);
        assert!(!watch.heard(MS * 9000, now + Duration::from_secs(60)), "asked again within the pause");
        assert!(watch.heard(MS * 9000, now + Duration::from_secs(121)));
    }

    /// One slow answer among quick ones is a large answer, not a slow circuit.
    #[test]
    fn one_slow_answer_is_not_a_slow_circuit() {
        let (mut watch, now) = (Watch::default(), Instant::now());
        for took in [600, 700, 30_000, 650, 620, 640] {
            assert!(!watch.heard(MS * took, now), "{took}");
        }
    }

    /// The answers that build new circuits are slow by nature, and are not counted.
    #[test]
    fn answers_over_new_circuits_settle_first() {
        let (mut watch, now) = (Watch::default(), Instant::now());
        watch.renewed();
        let asked: Vec<bool> = (0..SETTLING as usize + WINDOW).map(|_| watch.heard(MS * 9000, now)).collect();
        assert_eq!(asked.iter().filter(|asked| **asked).count(), 1);
        assert!(asked.last().copied().unwrap_or(false));
    }

    #[test]
    fn only_a_quick_quad9_means_the_network_works() {
        assert!(network_works(Some(MS * 800)));
        assert!(!network_works(Some(MS * 7000)));
        assert!(!network_works(None));
    }
}
