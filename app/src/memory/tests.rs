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

/// The parts come from one measurement of each process, so this process's part is never more than the whole, even
/// while the other tests take and give back memory.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[test]
fn the_figure_taken_apart_holds_this_process_within_it() {
    for _ in 0..50 {
        let parts = settled_parts().expect("the figure taken apart");
        assert!(parts.own_megabytes > 0, "{parts:?}");
        assert!(parts.own_megabytes <= parts.megabytes, "{parts:?}");
        assert!(parts.processes >= 1, "{parts:?}");
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
#[test]
fn the_whole_adds_up_every_process_and_the_part_is_this_one() {
    assert_eq!(added_up(7, &[(7, 10), (8, 20), (9, 30)]), Some((60, 10, 3)));
    assert_eq!(added_up(9, &[(7, 10), (8, 20), (9, 30)]), Some((60, 30, 3)));
    assert_eq!(added_up(7, &[(7, 10)]), Some((10, 10, 1)));
    // This process could not be measured: no figure, rather than one that says it takes nothing.
    assert_eq!(added_up(7, &[(8, 20)]), None);
    assert_eq!(added_up(7, &[]), None);
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

    /// Which processes are counted, not how much: the other tests of this program take and give back tens of MB
    /// meanwhile, and a figure compared with one taken earlier failed on CI for that alone.
    #[test]
    fn the_whole_figure_holds_this_process_and_stays_steady_when_asked_often() {
        let own = i32::try_from(process::id()).unwrap_or(0);
        for _ in 0..200 {
            assert!(counted().is_some_and(|pids| pids.contains(&own)), "the process itself went missing");
            assert!(used().is_some_and(|bytes| bytes > 0), "no figure");
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

/// On Linux a sandbox may stand between the program and WebKit's web process: everything under the program is found,
/// at any depth and through any name, and nothing that is not under it.
#[test]
fn processes_under_the_program_are_found_at_any_depth() {
    let processes = [(10, 1), (20, 10), (21, 20), (22, 21), (30, 10), (40, 99), (41, 40), (10, 10)];
    let mut found = descendants(10, &processes);
    found.sort_unstable();
    assert_eq!(found, vec![20, 21, 22, 30], "the program itself is not among what is ended");
    assert_eq!(descendants(7, &processes), Vec::<u32>::new());
    // A loop in what the system reported cannot keep the walk going.
    assert_eq!(descendants(1, &[(2, 3), (3, 2), (2, 1)]), vec![2, 3]);
}

#[test]
fn the_parent_is_read_after_the_name_whatever_the_name_holds() {
    assert_eq!(parent_in_stat("1234 (WebKitWebProces) S 42 1234 1234 0 -1"), Some(42));
    assert_eq!(parent_in_stat("1234 (a (b) c) R 7 1 1"), Some(7), "brackets and spaces in the name");
    assert_eq!(parent_in_stat("1234 (x) y) S 9 1"), Some(9), "a closing bracket inside the name");
    assert_eq!(parent_in_stat("1234 (gone"), None);
    assert_eq!(parent_in_stat("1234 (x) S"), None);
    assert_eq!(parent_in_stat("1234 (x) S -1"), None);
    assert_eq!(parent_in_stat(""), None);
}
