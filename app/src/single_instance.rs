//! One program at a time for each person on the machine. A second start hands over what it was started with and
//! leaves, which is how a `mailto:` link reaches a running app on Windows and Linux. On macOS the system does this
//! itself for the bundle, and this catches the program being started directly.
//!
//! The door between the two is the person's own, so that another account can neither take its name first, which made
//! the app hand that account its link and quit, nor knock on it. On macOS and Linux it is a socket in a folder no one
//! else may enter: `XDG_RUNTIME_DIR`, the temporary folder macOS gives each account, or else a folder of the app's own
//! made for this person alone. A folder anyone else may open is never used: interprocess's own choice, the shared
//! `/tmp` on macOS and an abstract name on Linux, was open to every account. On Windows every account sees every
//! pipe's name, so the name carries this person's security identifier, a start hands over only to a pipe served as the
//! same person, and Windows lets other accounts only read a pipe they did not make.

use crate::{guard, incoming};
use interprocess::local_socket::{ListenerOptions, Name, prelude::*};
use std::{
    io::{self, Read, Write},
    thread,
};

#[cfg(windows)]
mod user;

const NAME: &str = "org.koval-dev.proton-mail-light";
/// Any other program of this person can write here, so it is read as a link would be: with a limit.
const MOST_BYTES: u64 = 256 * 1024;

/// True once a running copy of this person's has what this start was given.
fn hand_over(name: Name<'_>, arguments: &[String]) -> bool {
    let Ok(mut stream) = LocalSocketStream::connect(name) else { return false };
    if !place::served_by_this_person(&stream) {
        log::warn!("the door of a running copy is another account's: nothing was handed over");
        return false;
    }
    stream.write_all(arguments.join("\n").as_bytes()).is_ok()
}

fn heard(stream: LocalSocketStream) -> Option<String> {
    let mut text = String::new();
    stream.take(MOST_BYTES).read_to_string(&mut text).ok()?;
    Some(text)
}

/// Opens the door, and hands `then` what each later start brings through it.
fn listen(name: Name<'static>, then: impl Fn(&str) + Send + 'static) -> io::Result<()> {
    let listener = ListenerOptions::new().name(name).try_overwrite(true).create_sync()?;
    thread::Builder::new().name("single-instance".to_string()).spawn(move || {
        for text in listener.incoming().flatten().filter_map(heard) {
            then(&text);
        }
    })?;
    Ok(())
}

/// False when another copy took over and this one should leave. A restart after a crash never
/// hands over: the copy it would hand over to is the one that is ending.
pub fn claim() -> bool {
    let Ok(name) = place::name() else {
        log::warn!("no door of this person's own: a second start runs beside this one");
        return true;
    };
    let arguments: Vec<String> = crate::program::arguments().collect();
    // A copy started by the one before it would only hand itself back to that one, which is leaving.
    if !guard::is_restart() && !guard::is_relaunch() && hand_over(name.borrow(), &arguments) {
        return false;
    }
    if listen(name, |text| incoming::second_start(text.lines())).is_err() {
        log::warn!("no second start can reach this one");
    }
    true
}

#[cfg(unix)]
mod place {
    use interprocess::local_socket::{GenericFilePath, Name, Stream, ToFsName as _};
    use std::{
        fs, io,
        os::unix::fs::{DirBuilderExt as _, PermissionsExt as _},
        path::{Path, PathBuf},
    };

    /// Any right at all for the folder's group or for anyone else.
    const OTHERS: u32 = 0o077;
    const OWN_FOLDER: &str = "instance";
    /// Short: the app's own folder lies deep, and macOS takes at most 103 bytes for a socket's path.
    const IN_OWN_FOLDER: &str = "door";

    /// A folder only its owner may enter, so that no one else can reach a socket in it or put one there first.
    pub fn is_private(folder: &Path) -> bool {
        fs::metadata(folder).is_ok_and(|about| about.is_dir() && about.permissions().mode() & OTHERS == 0)
    }

    /// `TMPDIR`. On macOS it names a folder of this account's own; elsewhere it is mostly the shared `/tmp`, which
    /// `is_private` refuses. A socket goes in it only once no one else may enter it.
    fn temporary() -> PathBuf {
        // nosemgrep: rust.lang.security.temp-dir.temp-dir
        std::env::temp_dir()
    }

    /// A folder of the app's own, made for this person alone, for a system that offers none.
    fn own_folder() -> Option<PathBuf> {
        let folder = dirs::data_dir()?.join(crate::guard::IDENTIFIER).join(OWN_FOLDER);
        fs::DirBuilder::new().recursive(true).mode(0o700).create(&folder).ok()?;
        Some(folder)
    }

    /// The door's path: in the first private folder the system offers, else in the app's own, else nowhere.
    pub fn door(system: impl IntoIterator<Item = PathBuf>, own: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
        let shared = system.into_iter().find(|folder| is_private(folder));
        shared
            .map(|folder| folder.join(format!("{}.sock", super::NAME)))
            .or_else(|| own().filter(|folder| is_private(folder)).map(|folder| folder.join(IN_OWN_FOLDER)))
    }

    pub fn name() -> io::Result<Name<'static>> {
        let system = [dirs::runtime_dir(), Some(temporary())].into_iter().flatten();
        door(system, own_folder)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?
            .to_fs_name::<GenericFilePath>()
    }

    /// No one else may enter the folder, so whoever serves the door is this person.
    pub const fn served_by_this_person(_: &Stream) -> bool {
        true
    }
}

#[cfg(windows)]
mod place {
    use super::user;
    use interprocess::local_socket::{GenericNamespaced, Name, Stream, ToNsName as _, traits::StreamCommon as _};
    use std::{fmt::Write as _, io};

    /// The pipe's name with the person's security identifier in it, so that two people on one machine each have one.
    pub fn named_for(person: &[u8]) -> String {
        let mut name = format!("{}.", super::NAME);
        for byte in person {
            let _ = write!(name, "{byte:02x}");
        }
        name
    }

    pub fn name() -> io::Result<Name<'static>> {
        let person = user::own().ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        named_for(&person).to_ns_name::<GenericNamespaced>()
    }

    /// Whether the copy on the other side runs as this person: any account may make a pipe of any name, this one too.
    pub fn served_by_this_person(stream: &Stream) -> bool {
        let server = stream.peer_creds().ok().and_then(|credentials| credentials.pid());
        let theirs = server.and_then(user::of_process);
        theirs.is_some() && theirs == user::own()
    }
}

/// Neither: a second start runs beside the first.
#[cfg(not(any(unix, windows)))]
mod place {
    use interprocess::local_socket::{Name, Stream};
    use std::io;

    pub fn name() -> io::Result<Name<'static>> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub const fn served_by_this_person(_: &Stream) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
