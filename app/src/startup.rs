//! Starting with the system.

use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use std::path::{Path, PathBuf};

const NAME: &str = "Proton Mail Light";
/// Passed by the login item, so that the app starts in the tray.
const HIDDEN: &str = "--hidden";

pub fn started_hidden() -> bool {
    std::env::args().any(|argument| argument == HIDDEN)
}

/// On macOS the thing to start is the bundle around the program.
fn bundle_of(program: &Path) -> PathBuf {
    program.ancestors().find(|path| path.extension().is_some_and(|extension| extension == "app")).unwrap_or(program).to_path_buf()
}

fn launcher() -> Option<AutoLaunch> {
    let program = std::env::current_exe().ok()?;
    let path = bundle_of(&program);
    let mut builder = AutoLaunchBuilder::new();
    builder.set_app_name(NAME).set_app_path(&path.to_string_lossy()).set_args(&[HIDDEN]);
    #[cfg(target_os = "macos")]
    builder.set_macos_launch_mode(auto_launch::MacOSLaunchMode::LaunchAgent);
    builder.build().ok()
}

pub fn enabled() -> bool {
    launcher().is_some_and(|launcher| launcher.is_enabled().unwrap_or(false))
}

pub fn set(on: bool) -> bool {
    launcher().is_some_and(|launcher| if on { launcher.enable() } else { launcher.disable() }.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_in_a_bundle_starts_as_the_bundle() {
        let program = Path::new("/Applications/Proton Mail Light.app/Contents/MacOS/proton-mail-light");
        assert_eq!(bundle_of(program), Path::new("/Applications/Proton Mail Light.app"));
    }

    #[test]
    fn a_lone_program_starts_as_itself() {
        assert_eq!(bundle_of(Path::new("/opt/proton-mail-light")), Path::new("/opt/proton-mail-light"));
    }
}
