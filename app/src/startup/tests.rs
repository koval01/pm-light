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

#[cfg(unix)]
mod places {
    use super::*;
    use std::{
        fs,
        os::unix::fs::PermissionsExt as _,
        sync::atomic::{AtomicUsize, Ordering},
    };

    static MADE: AtomicUsize = AtomicUsize::new(0);

    /// Where the tests make their folders: the first that no one else may write on the way to. The temporary folder
    /// macOS gives each account is one, the shared `/tmp` of Linux is not, and neither is a checkout below it.
    fn base() -> PathBuf {
        // Only for tests: folders of their own, which they take away again.
        let places = [std::env::temp_dir(), dirs::home_dir().unwrap()];
        places.into_iter().find(|place| !others_may_change(place)).expect("a folder no one else writes")
    }

    /// A folder of a test's own, with exactly these rights.
    struct Folder(PathBuf);

    impl Folder {
        fn new(mode: u32) -> Self {
            let made = MADE.fetch_add(1, Ordering::Relaxed);
            let path = base().join(format!(".pm-light-startup-test-{}-{made}", std::process::id()));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            Self(path)
        }

        /// A program in it, which only its owner may change.
        fn program(&self) -> PathBuf {
            let program = self.0.join("proton-mail-light");
            fs::write(&program, "").unwrap();
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
            program
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_persons_own_home_is_no_shared_folder() {
        assert!(!others_may_change(&dirs::home_dir().unwrap()));
    }

    #[test]
    fn a_program_in_a_folder_only_its_owner_writes_may_start_with_the_system() {
        for mode in [0o755, 0o700, 0o775] {
            let folder = Folder::new(mode);
            assert!(!others_may_change(&folder.program()), "{mode:o}");
        }
    }

    #[test]
    fn a_program_in_a_folder_anyone_writes_may_not() {
        for mode in [0o777, 0o1777, 0o703] {
            let folder = Folder::new(mode);
            assert!(others_may_change(&folder.program()), "{mode:o}");
        }
    }

    #[test]
    fn a_folder_anyone_writes_further_up_counts_too() {
        let shared = Folder::new(0o1777);
        let inner = shared.0.join("Proton Mail Light.app");
        fs::create_dir(&inner).unwrap();
        fs::set_permissions(&inner, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(others_may_change(&inner));
        assert!(others_may_change(Path::new("/tmp")));
    }

    #[test]
    fn a_program_anyone_may_write_may_not() {
        let folder = Folder::new(0o755);
        let program = folder.program();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o757)).unwrap();
        assert!(others_may_change(&program));
    }

    #[test]
    fn a_link_is_followed_to_where_the_program_really_is() {
        let (own, shared) = (Folder::new(0o755), Folder::new(0o777));
        let program = shared.program();
        let link = own.0.join("proton-mail-light");
        std::os::unix::fs::symlink(&program, &link).unwrap();
        assert!(others_may_change(&link));
    }

    #[test]
    fn what_cannot_be_read_counts_as_shared() {
        let folder = Folder::new(0o755);
        assert!(others_may_change(&folder.0.join("missing")));
    }
}
