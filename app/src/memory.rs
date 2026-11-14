//! How much memory the app takes, for the footer. The figure is meant to be the one a system monitor
//! would add up, so it counts the webview's own processes too: a light client must not look lighter
//! than it is by leaving out what it started. Tor runs inside the program, and is in the figure.

use crate::App;
use slint::{ComponentHandle, Timer, TimerMode};
use std::{cell::RefCell, time::Duration};

const EVERY: Duration = Duration::from_secs(10);
const SETTLED: Duration = Duration::from_secs(3);
const BYTES_IN_A_MEGABYTE: u64 = 1024 * 1024;

thread_local! {
    static TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
}

#[cfg(not(checked_memory))]
unsafe extern "C" {
    /// From `mimalloc`, which the program is linked with.
    fn mi_collect(force: bool);
}

/// Hands what this thread has freed back to the system, for a thread with nothing left to do.
#[cfg(not(checked_memory))]
pub fn give_back() {
    // Safety: takes no pointers, and only tidies the calling thread's own heap.
    unsafe { mi_collect(true) };
}

/// A build for AddressSanitizer runs on the system's allocator, which gives back by itself.
#[cfg(checked_memory)]
pub const fn give_back() {}

/// The system's libraries use their own heap, which keeps holes it could return.
#[cfg(target_os = "macos")]
fn give_back_system_heap() {
    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // Safety: no zone means all of them, and a goal of zero means as much as there is.
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

#[cfg(not(target_os = "macos"))]
const fn give_back_system_heap() {}

/// The figure the footer shows, after the heaps gave back what they could: for the account check's memory rounds.
pub fn settled_megabytes() -> Option<u64> {
    give_back();
    give_back_system_heap();
    system::used().map(megabytes)
}

const fn megabytes(bytes: u64) -> u64 {
    (bytes + BYTES_IN_A_MEGABYTE / 2) / BYTES_IN_A_MEGABYTE
}

#[cfg(target_os = "macos")]
mod system {
    use std::{ffi::c_void, mem::MaybeUninit, process};

    const RUSAGE_INFO_V0: i32 = 0;
    const MOST_PROCESSES: usize = 8192;
    const NAME_BYTES: usize = 64;
    const WEBKIT: &[u8] = b"com.apple.WebKit";

    /// `rusage_info_v0` of `<sys/resource.h>`.
    #[repr(C)]
    struct Usage {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        idle_wakeups: u64,
        interrupt_wakeups: u64,
        page_ins: u64,
        wired_size: u64,
        resident_size: u64,
        footprint: u64,
        started: u64,
        ended: u64,
    }

    unsafe extern "C" {
        fn proc_listallpids(buffer: *mut c_void, size: i32) -> i32;
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut c_void) -> i32;
        fn proc_name(pid: i32, buffer: *mut c_void, size: u32) -> i32;
        fn responsibility_get_pid_responsible_for_pid(pid: i32) -> i32;
    }

    pub(super) fn footprint(pid: i32) -> Option<u64> {
        let mut usage = MaybeUninit::<Usage>::uninit();
        // Safety: the buffer has the size this flavor writes, and is read only if the call succeeded.
        unsafe {
            (proc_pid_rusage(pid, RUSAGE_INFO_V0, usage.as_mut_ptr().cast()) == 0)
                .then(|| usage.assume_init().footprint)
        }
    }

    pub(super) fn is_webkit(pid: i32) -> bool {
        let mut name = [0_u8; NAME_BYTES];
        // Safety: the buffer has the size given, and the call writes no more.
        let length = unsafe { proc_name(pid, name.as_mut_ptr().cast(), u32::try_from(NAME_BYTES).unwrap_or(0)) };
        usize::try_from(length).is_ok_and(|length| name.get(..length).is_some_and(|name| name.starts_with(WEBKIT)))
    }

    /// The physical footprint, which is what Activity Monitor calls Memory, of this program and of
    /// WebKit's processes that belong to it. The system ties those to whoever is responsible for the
    /// program, which is the program itself from the Dock, and the terminal when started from one,
    /// so the name is checked too: a terminal may be responsible for much more.
    pub fn used() -> Option<u64> {
        let own = i32::try_from(process::id()).ok()?;
        // Safety: takes a number and only reads system state.
        let responsible = |pid: i32| unsafe { responsibility_get_pid_responsible_for_pid(pid) };
        let ours = responsible(own);
        let mut pids = vec![0_i32; MOST_PROCESSES];
        let size = i32::try_from(pids.len() * size_of::<i32>()).ok()?;
        // Safety: the buffer holds `size` bytes, and the call writes no more.
        let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), size) };
        pids.truncate(usize::try_from(count).ok()?);
        let related = pids.into_iter().filter(|pid| *pid == own || (responsible(*pid) == ours && is_webkit(*pid)));
        Some(related.filter_map(footprint).sum())
    }
}

#[cfg(target_os = "linux")]
mod system {
    const PAGE: u64 = 4096;

    /// The program's resident pages. WebKit's processes are its children here, and are not counted.
    pub fn used() -> Option<u64> {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        Some(statm.split_whitespace().nth(1)?.parse::<u64>().ok()? * PAGE)
    }
}

/// The processes to count on Windows: this one, and under it, at any depth, WebView2's. Its browser process is a
/// child of the program and its renderers are the browser's children, all under one name. Anything else the
/// program started, or a process that took over a number once used, is left out by the name.
#[cfg(any(target_os = "windows", test))]
fn counted(own: u32, processes: &[(u32, u32, String)], is_webview: impl Fn(&str) -> bool) -> Vec<u32> {
    let mut found = vec![own];
    let mut index = 0;
    while let Some(&parent) = found.get(index) {
        for (pid, under, name) in processes {
            if *under == parent && !found.contains(pid) && is_webview(name) {
                found.push(*pid);
            }
        }
        index += 1;
    }
    found
}

#[cfg(target_os = "windows")]
mod system {
    use std::{mem::size_of, process};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
            },
            ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX2},
            Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };

    const WEBVIEW: &str = "msedgewebview2.exe";

    /// Every process: its number, its parent's, and the name of its program.
    pub(super) fn processes() -> Vec<(u32, u32, String)> {
        // Safety: takes flags and a number, and gives a handle that is closed below.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entry =
            PROCESSENTRY32W { dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0), ..Default::default() };
        let mut found = Vec::new();
        // Safety: the entry is as large as `dwSize` says, which the call checks.
        let mut more = unsafe { Process32FirstW(snapshot, &raw mut entry) } != 0;
        while more {
            let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(entry.szExeFile.get(..length).unwrap_or_default());
            found.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
            // Safety: as above.
            more = unsafe { Process32NextW(snapshot, &raw mut entry) } != 0;
        }
        // Safety: the handle came from the snapshot above and is closed once.
        unsafe { CloseHandle(snapshot) };
        found
    }

    /// The private working set, which is what Task Manager calls Memory.
    pub(super) fn private_working_set(pid: u32) -> Option<u64> {
        let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS_EX2>()).ok()?;
        // Safety: takes a right and a number; the handle is closed below.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 { cb: size, ..Default::default() };
        // Safety: the buffer is as large as `size`, which the call is told and does not exceed.
        let read =
            unsafe { K32GetProcessMemoryInfo(process, (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>(), size) }
                != 0;
        // Safety: the handle came from `OpenProcess` above and is closed once.
        unsafe { CloseHandle(process) };
        if read { u64::try_from(counters.PrivateWorkingSetSize).ok() } else { None }
    }

    pub fn used() -> Option<u64> {
        let own = process::id();
        let pids = super::counted(own, &processes(), |name| name.eq_ignore_ascii_case(WEBVIEW));
        let total: u64 = pids.into_iter().filter_map(private_working_set).sum();
        (total > 0).then_some(total)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod system {
    pub const fn used() -> Option<u64> {
        None
    }
}

fn show(app: &App) {
    if let Some(bytes) = system::used() {
        app.set_memory_megabytes(i32::try_from(megabytes(bytes)).unwrap_or(i32::MAX));
    }
}

pub fn start(app: &App) {
    show(app);
    let weak = app.as_weak();
    // The very first figure comes before there is any mail to hold.
    Timer::single_shot(SETTLED, {
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                show(&app);
            }
        }
    });
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, EVERY, move || {
        give_back();
        give_back_system_heap();
        if let Some(app) = weak.upgrade().filter(|app| app.window().is_visible()) {
            show(&app);
        }
    });
    TIMER.set(Some(timer));
}

#[cfg(test)]
mod tests;
