//! The hardening on Unix. Nothing here checks it on Windows.
#![cfg(unix)]

use super::*;

#[test]
fn a_crash_leaves_no_core_file() {
    assert_eq!(harden_process(), [] as [&str; 0]);
    let mut limit = libc::rlimit { rlim_cur: 1, rlim_max: 1 };
    // Safety: reads a limit of this process into a structure alive for the call.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &raw mut limit) }, 0);
    assert_eq!((limit.rlim_cur, limit.rlim_max), (0, 0));
}

#[cfg(all(target_os = "linux", not(checked_memory)))]
#[test]
fn no_one_else_may_read_the_process() {
    assert_eq!(harden_process(), [] as [&str; 0]);
    let unused: libc::c_ulong = 0;
    // Safety: asks a setting of this process, each argument of the width the call reads.
    assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE, unused, unused, unused, unused) }, 0);
}
