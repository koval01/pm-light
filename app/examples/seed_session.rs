//! Puts the session in `LIVE_SESSION` where the app keeps one, in the system's own store: the keychain, Windows'
//! credentials or the Secret Service. For CI's account check, on a machine that is thrown away after the run.
//! Prints nothing of the session. Run it with `cargo run -p proton-mail-light --example seed_session`.

use keyring::Entry;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(session) = std::env::var("LIVE_SESSION").ok().filter(|value| !value.trim().is_empty()) else {
        println!("no session in LIVE_SESSION");
        return ExitCode::FAILURE;
    };
    let stored = Entry::new("org.koval-dev.proton-mail-light", "session").and_then(|entry| entry.set_password(&session));
    match stored {
        Ok(()) => {
            println!("the session is in the system's store ({} bytes)", session.len());
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!("the session could not be stored: {}", kind(&error));
            ExitCode::FAILURE
        }
    }
}

const fn kind(error: &keyring::Error) -> &'static str {
    match error {
        keyring::Error::PlatformFailure(_) => "the platform failed",
        keyring::Error::NoStorageAccess(_) => "no access to the store",
        keyring::Error::NoEntry => "no entry",
        keyring::Error::BadEncoding(_) => "bad encoding",
        keyring::Error::TooLong(..) => "too long",
        keyring::Error::Invalid(..) => "invalid",
        keyring::Error::Ambiguous(_) => "ambiguous",
        _ => "other",
    }
}
