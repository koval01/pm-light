use super::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

static MADE: AtomicUsize = AtomicUsize::new(0);

/// A name no other test and no running copy uses.
fn unique(what: &str) -> String {
    format!("pm-light-test-{what}-{}-{}", std::process::id(), MADE.fetch_add(1, Ordering::Relaxed))
}

#[cfg(unix)]
mod folders {
    use super::{place, unique};
    use std::{
        fs,
        os::unix::fs::PermissionsExt as _,
        path::{Path, PathBuf},
    };

    /// A folder with exactly these rights, gone when the test ends.
    pub struct Folder(pub PathBuf);

    impl Folder {
        pub fn new(mode: u32) -> Self {
            // Only for tests: under the temporary folder, by a name of this test's own.
            let path = std::env::temp_dir().join(unique("folder"));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn private(path: &Path) -> bool {
        place::is_private(path)
    }

    #[test]
    fn a_folder_only_its_owner_may_enter_is_private() {
        assert!(private(&Folder::new(0o700).0));
    }

    #[test]
    fn a_folder_others_may_open_is_not() {
        for mode in [0o1777, 0o777, 0o755, 0o750, 0o730, 0o710, 0o705, 0o703, 0o701] {
            assert!(!private(&Folder::new(mode).0), "{mode:o} let someone else in");
        }
    }

    #[test]
    fn the_shared_temporary_folder_is_not() {
        assert!(!private(Path::new("/tmp")));
    }

    #[test]
    fn a_file_or_nothing_is_no_folder() {
        let folder = Folder::new(0o700);
        let file = folder.0.join("file");
        fs::write(&file, "").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(!private(&file));
        assert!(!private(&folder.0.join("missing")));
    }

    #[test]
    fn the_door_is_in_the_first_private_folder_the_system_offers() {
        let (shared, first, second) = (Folder::new(0o1777), Folder::new(0o700), Folder::new(0o700));
        let door = place::door([shared.0.clone(), first.0.clone(), second.0.clone()], || None);
        assert_eq!(door, Some(first.0.join("org.koval-dev.proton-mail-light.sock")));
    }

    #[test]
    fn with_no_private_folder_of_the_systems_it_is_in_the_apps_own() {
        let (shared, own) = (Folder::new(0o777), Folder::new(0o700));
        let door = place::door([shared.0.clone(), PathBuf::from("/tmp")], || Some(own.0.clone()));
        assert_eq!(door, Some(own.0.join("door")));
    }

    #[test]
    fn an_own_folder_others_may_open_is_no_place_either() {
        let (shared, own) = (Folder::new(0o777), Folder::new(0o755));
        assert_eq!(place::door([shared.0.clone()], || Some(own.0.clone())), None);
        assert_eq!(place::door([], || None), None);
    }

    /// macOS gives each account a temporary folder of its own, which is where the door is there.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_temporary_folder_macos_gives_an_account_is_private() {
        let temporary = std::env::temp_dir();
        assert!(
            temporary.starts_with("/var/folders") || temporary.starts_with("/private/var/folders"),
            "{temporary:?}"
        );
        assert!(private(&temporary));
        // A socket's path takes at most 103 bytes there.
        assert!(temporary.join("org.koval-dev.proton-mail-light.sock").as_os_str().len() < 104);
    }
}

/// A door of the test's own: in a private folder on macOS and Linux, under a name of its own on Windows.
#[cfg(unix)]
fn test_door() -> (Name<'static>, folders::Folder) {
    use interprocess::local_socket::{GenericFilePath, ToFsName as _};
    let folder = folders::Folder::new(0o700);
    (folder.0.join("door").to_fs_name::<GenericFilePath>().unwrap(), folder)
}

#[cfg(windows)]
fn test_door() -> (Name<'static>, ()) {
    use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
    (unique("door").to_ns_name::<GenericNamespaced>().unwrap(), ())
}

#[test]
fn a_second_start_hands_its_arguments_to_the_first() {
    let (name, _folder) = test_door();
    let (sender, received) = mpsc::channel();
    listen(name.clone(), move |text| sender.send(text.to_string()).unwrap()).unwrap();
    let given = ["mailto:someone@example.com?subject=Hello".to_string(), "--hidden".to_string()];
    assert!(hand_over(name, &given));
    let text = received.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(text.lines().collect::<Vec<_>>(), given);
}

#[test]
fn with_no_one_at_the_door_nothing_is_handed_over() {
    let (name, _folder) = test_door();
    assert!(!hand_over(name, &["mailto:someone@example.com".to_string()]));
}

#[test]
fn more_than_the_limit_is_cut_off() {
    let (name, _folder) = test_door();
    let (sender, received) = mpsc::channel();
    listen(name.clone(), move |text| sender.send(text.len()).unwrap()).unwrap();
    let long = "a".repeat(usize::try_from(MOST_BYTES).unwrap() + 1000);
    // The copy that listens stops reading at the limit, so the writer may or may not see all of it go.
    let _ = hand_over(name, &[long]);
    assert_eq!(received.recv_timeout(Duration::from_secs(10)).unwrap(), usize::try_from(MOST_BYTES).unwrap());
}

#[cfg(windows)]
mod windows {
    use super::{place, user};

    #[test]
    fn this_program_runs_as_someone() {
        let own = user::own().unwrap();
        // A security identifier starts with its revision, which is 1, and holds at least eight bytes.
        assert_eq!(own.first(), Some(&1));
        assert!(own.len() >= 8);
    }

    #[test]
    fn this_process_is_known_as_the_same_person() {
        assert_eq!(user::of_process(std::process::id()), user::own());
    }

    #[test]
    fn the_system_is_someone_else() {
        // The System process: either it cannot be asked, or it runs as Local System.
        assert_ne!(user::of_process(4), user::own());
        assert_eq!(user::of_process(u32::MAX - 3), None);
    }

    #[test]
    fn each_person_has_a_pipe_of_their_own() {
        let (one, other) = (place::named_for(&[1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0]), place::named_for(&[1, 2, 0xab]));
        assert_eq!(one, "org.koval-dev.proton-mail-light.010100000000000512000000");
        assert_eq!(other, "org.koval-dev.proton-mail-light.0102ab");
        assert_ne!(one, other);
    }
}
