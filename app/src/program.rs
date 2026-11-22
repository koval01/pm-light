//! This program as it was started: the words it was given after its own name, and the file it runs from. Both come
//! from whoever started it, with that person's own rights. They switch on the checks CI runs, carry a `mailto:` link,
//! or say how a start came about, and decide nothing about what may be trusted: a check makes the core refuse every
//! request but a read, a link only fills in a letter the person still has to send, and the file is only started again
//! as the same person.

use std::path::PathBuf;

/// What the program was given after its own name. The first word is whatever its starter chose to call it, so it is
/// never read as a switch.
pub fn arguments() -> impl Iterator<Item = String> {
    // The one place the arguments are read, each by a check of its own: see the top of this file.
    // nosemgrep: rust.lang.security.args.args
    after_own_name(std::env::args())
}

fn after_own_name(all: impl Iterator<Item = String>) -> impl Iterator<Item = String> {
    all.skip(1)
}

/// The file this program runs from, to start it again after a panic or on purpose, and for the login item. Anyone who
/// can change what it names can already run anything as this person, and it is started with no more rights than this
/// copy has. `startup.rs` refuses a login item in a folder other people can write.
pub fn file() -> Option<PathBuf> {
    // nosemgrep: rust.lang.security.current-exe.current-exe
    std::env::current_exe().ok()
}

#[cfg(test)]
mod tests;
