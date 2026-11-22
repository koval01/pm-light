//! Ends the web engine's page processes, for the picture check alone: on CI there is no person to end them in Task
//! Manager or Activity Monitor, so the check does it once the page has loaded, and the picture must come back by
//! itself. Nothing else in the app ends a process.

/// Ends every page process of the engine, and says how many it ended.
pub fn end_page_processes() -> usize {
    crate::memory::page_processes().into_iter().filter(|pid| end(*pid)).count()
}

#[cfg(unix)]
fn end(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    // Safety: takes two numbers; the process is one of the program's own engine, found just before.
    unsafe { libc::kill(pid, libc::SIGKILL) == 0 }
}

#[cfg(windows)]
fn end(pid: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
    };
    // Safety: takes a right and a number; the handle is closed below.
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if process.is_null() {
        return false;
    }
    // Safety: a handle opened above with the right to end the process.
    let ended = unsafe { TerminateProcess(process, 1) } != 0;
    // Safety: the handle came from `OpenProcess` above and is closed once.
    unsafe { CloseHandle(process) };
    ended
}

#[cfg(not(any(unix, windows)))]
const fn end(_pid: u32) -> bool {
    false
}
