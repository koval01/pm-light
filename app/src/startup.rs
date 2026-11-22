//! Starting with the system.

use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use std::path::{Path, PathBuf};

const NAME: &str = "Proton Mail Light";
/// Passed by the login item, so that the app starts in the tray.
const HIDDEN: &str = "--hidden";

pub fn started_hidden() -> bool {
    crate::program::arguments().any(|argument| argument == HIDDEN)
}

/// On macOS the thing to start is the bundle around the program.
fn bundle_of(program: &Path) -> PathBuf {
    program
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .unwrap_or(program)
        .to_path_buf()
}

/// Where the login item starts the app from: the bundle on macOS, the program elsewhere.
fn place() -> Option<PathBuf> {
    Some(bundle_of(&crate::program::file()?))
}

fn launcher(path: &Path) -> Option<AutoLaunch> {
    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name(NAME).set_app_path(&path.to_string_lossy()).set_args(&[HIDDEN]);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    builder.build().ok()
}

/// Whether another account may change what is at `path`, or put something there once it is gone: a folder on the way
/// that anyone may write to, as `/tmp` or `/Users/Shared`, which a restart may also empty. The login item would then
/// start their program as this person. What cannot be read is taken for such a place.
#[cfg(unix)]
fn others_may_change(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    /// Writing, for anyone at all.
    const ANYONE_WRITES: u32 = 0o002;
    let Ok(path) = std::fs::canonicalize(path) else { return true };
    path.ancestors()
        .any(|step| std::fs::metadata(step).map_or(true, |about| about.permissions().mode() & ANYONE_WRITES != 0))
}

/// Windows grants by lists of rights, not read here: a program installed for all users or in this person's own
/// folders is not another account's to change.
#[cfg(not(unix))]
const fn others_may_change(_path: &Path) -> bool {
    false
}

pub fn enabled() -> bool {
    place().and_then(|path| launcher(&path)).is_some_and(|launcher| launcher.is_enabled().unwrap_or(false))
}

pub fn set(on: bool) -> bool {
    let Some(path) = place() else { return false };
    if on && others_may_change(&path) {
        log::warn!("no login item: the app runs from a folder another account may write to");
        return false;
    }
    launcher(&path).is_some_and(|launcher| if on { launcher.enable() } else { launcher.disable() }.is_ok())
}

#[cfg(test)]
mod tests;
