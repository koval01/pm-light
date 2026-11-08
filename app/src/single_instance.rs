//! One program at a time. A second start hands over what it was started with and leaves, which is how
//! a `mailto:` link reaches a running app on Windows and Linux. On macOS the system does this itself
//! for the bundle, and this catches the program being started directly.

use crate::{guard, incoming};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Name, NameType, ToFsName, ToNsName, prelude::*,
};
use std::{
    io::{self, Read, Write},
    thread,
};

const NAME: &str = "org.koval-dev.proton-mail-light.sock";
/// Any other program of this user can write here, so it is read as a link would be: with a limit.
const MOST_BYTES: u64 = 256 * 1024;

fn name() -> io::Result<Name<'static>> {
    if GenericNamespaced::is_supported() {
        NAME.to_ns_name::<GenericNamespaced>()
    } else {
        std::env::temp_dir().join(NAME).to_fs_name::<GenericFilePath>()
    }
}

fn hand_over(arguments: &[String]) -> bool {
    let Some(mut stream) = name().ok().and_then(|name| LocalSocketStream::connect(name).ok()) else { return false };
    stream.write_all(arguments.join("\n").as_bytes()).is_ok()
}

fn heard(stream: LocalSocketStream) {
    let mut text = String::new();
    if stream.take(MOST_BYTES).read_to_string(&mut text).is_err() {
        return;
    }
    incoming::second_start(text.lines());
}

fn listen() -> io::Result<()> {
    let listener = ListenerOptions::new().name(name()?).try_overwrite(true).create_sync()?;
    thread::Builder::new().name("single-instance".to_string()).spawn(move || {
        for stream in listener.incoming().flatten() {
            heard(stream);
        }
    })?;
    Ok(())
}

/// False when another copy took over and this one should leave. A restart after a crash never
/// hands over: the copy it would hand over to is the one that is ending.
pub fn claim() -> bool {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    // A copy started by the one before it would only hand itself back to that one, which is leaving.
    if !guard::is_restart() && !guard::is_relaunch() && hand_over(&arguments) {
        return false;
    }
    if listen().is_err() {
        log::warn!("no second start can reach this one");
    }
    true
}
