//! A check run by a machine: the account check, its tour of the screens, the long run of random clicks. It may click
//! anything, and nothing may leave the window for good. What would wait on a person, a question, a file to pick, the
//! browser or the printer, is refused with a word in the journal, as is quitting or starting again. The account cannot
//! change either: the core refuses every request but a read (`Service::forbid_changes`).

/// Whether this start is a check.
pub fn running() -> bool {
    crate::account_check::asked()
}

/// True when `what` is refused, because a check is running; the journal says so.
pub fn refuses(what: &str) -> bool {
    let refused = running();
    if refused {
        log::info!("{what}: refused, a check waits on no one");
    }
    refused
}
