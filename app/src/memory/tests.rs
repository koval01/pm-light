use super::*;

#[test]
fn megabytes_are_rounded() {
    assert_eq!(megabytes(0), 0);
    assert_eq!(megabytes(BYTES_IN_A_MEGABYTE), 1);
    assert_eq!(megabytes(BYTES_IN_A_MEGABYTE * 3 / 2), 2);
    assert_eq!(megabytes(BYTES_IN_A_MEGABYTE * 71 + 10), 71);
}

/// WebView2's browser under the program, its renderers under the browser: all counted. Another program the
/// app started, a WebView2 of someone else's, and a process whose parent's number is the program's but whose
/// name is not WebView2's are not.
#[test]
fn webview2_under_the_program_is_counted_and_nothing_else() {
    let webview = |name: &str| name.eq_ignore_ascii_case("msedgewebview2.exe");
    let processes = [
        (10, 1, "proton-mail-light.exe".to_string()),
        (20, 10, "msedgewebview2.exe".to_string()),
        (21, 20, "msedgewebview2.exe".to_string()),
        (22, 20, "MSEDGEWEBVIEW2.EXE".to_string()),
        (30, 10, "proton-mail-light.exe".to_string()),
        (40, 99, "msedgewebview2.exe".to_string()),
        (41, 40, "msedgewebview2.exe".to_string()),
        (50, 21, "notepad.exe".to_string()),
    ];
    let mut found = counted(10, &processes, webview);
    found.sort_unstable();
    assert_eq!(found, vec![10, 20, 21, 22]);
    assert_eq!(counted(7, &[], webview), vec![7]);
    // A loop in the parents, which a reused number can make, ends.
    let looped = [(1, 2, "msedgewebview2.exe".to_string()), (2, 1, "msedgewebview2.exe".to_string())];
    assert_eq!(counted(1, &looped, webview), vec![1, 2]);
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn this_program_is_found_to_use_memory() {
    assert!(system::used().is_some_and(|bytes| bytes > BYTES_IN_A_MEGABYTE));
}

/// The calls behind `unsafe` on macOS given numbers no process has, and run often, for the address sanitizer to watch.
#[cfg(target_os = "macos")]
mod macos {
    use super::system::*;
    use std::process;

    #[test]
    fn this_process_has_a_footprint_and_no_process_has_none() {
        let own = i32::try_from(process::id()).unwrap_or(0);
        assert!(footprint(own).is_some_and(|bytes| bytes > 0));
        for nobody in [-1, i32::MIN, i32::MAX, 999_999_999] {
            assert_eq!(footprint(nobody), None, "pid {nobody}");
            assert!(!is_webkit(nobody), "pid {nobody}");
        }
        assert!(!is_webkit(own), "the test is no WebKit process");
    }

    #[test]
    fn the_whole_figure_holds_this_process_and_stays_steady_when_asked_often() {
        let own = i32::try_from(process::id()).unwrap_or(0);
        let alone = footprint(own).unwrap_or(0);
        for _ in 0..200 {
            assert!(used().is_some_and(|bytes| bytes >= alone / 2), "the process itself went missing");
        }
    }
}

/// The calls behind `unsafe` on Windows given numbers no process has, and run often.
#[cfg(target_os = "windows")]
mod windows {
    use super::system::*;
    use std::process;

    #[test]
    fn the_list_of_processes_holds_this_one_by_name() {
        let own = process::id();
        let all = processes();
        assert!(all.iter().any(|(pid, _, name)| *pid == own && name.to_ascii_lowercase().ends_with(".exe")));
        assert!(all.iter().all(|(_, _, name)| !name.contains('\0')), "a name kept its end mark");
    }

    #[test]
    fn this_process_has_a_working_set_and_no_process_has_none() {
        assert!(private_working_set(process::id()).is_some_and(|bytes| bytes > 0));
        // 0 is the idle process, which cannot be opened; the others are no process at all.
        for nobody in [0, u32::MAX, 0xFFFF_FFF0] {
            assert_eq!(private_working_set(nobody), None, "pid {nobody}");
        }
    }

    #[test]
    fn asked_often_the_figure_stays() {
        for _ in 0..100 {
            assert!(used().is_some_and(|bytes| bytes > 0));
        }
    }
}
