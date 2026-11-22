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
    // Safety: takes no pointers. Forced, it also tidies what ended threads left behind and the arenas the whole program
    // shares, which mimalloc guards itself, so any thread may call it at any time.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
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
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
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

/// The figure taken apart, for the long run: all of it, this process's own part, and how many processes it adds up. A
/// leak in the program grows its own part; a webview's caches grow the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parts {
    pub megabytes: u64,
    pub own_megabytes: u64,
    pub processes: usize,
}

/// `settled_megabytes`, taken apart.
pub fn settled_parts() -> Option<Parts> {
    give_back();
    give_back_system_heap();
    let (total, own, processes) = system::parts()?;
    Some(Parts { megabytes: megabytes(total), own_megabytes: megabytes(own), processes })
}

/// The whole and this process's part, each process measured once, so the part is never more than the whole.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn added_up<P: PartialEq + Copy>(own: P, measured: &[(P, u64)]) -> Option<(u64, u64, usize)> {
    let mine = measured.iter().find(|(pid, _)| *pid == own)?.1;
    Some((measured.iter().map(|(_, bytes)| bytes).sum(), mine, measured.len()))
}

/// The engine's processes that draw pages, for a check that ends them to see the pages come back: WebKit's web content
/// processes on macOS, WebView2's browser processes under the program on Windows, whose renderers go with them, and
/// WebKitGTK's web processes on Linux.
pub fn page_processes() -> Vec<u32> {
    system::page_processes()
}

/// Every process under `own`, at any depth, whatever it is: on Linux a sandbox may stand between the program and
/// WebKit's processes.
#[cfg(any(target_os = "linux", test))]
fn descendants(own: u32, processes: &[(u32, u32)]) -> Vec<u32> {
    let mut found = vec![own];
    let mut index = 0;
    while let Some(&parent) = found.get(index) {
        for (pid, under) in processes {
            if *under == parent && !found.contains(pid) {
                found.push(*pid);
            }
        }
        index += 1;
    }
    found.retain(|pid| *pid != own);
    found
}

/// The parent's number in a line of `/proc/<pid>/stat`. The name in brackets may hold brackets and spaces itself, so
/// the fields are read after the last closing one: the state, then the parent.
#[cfg(any(target_os = "linux", test))]
fn parent_in_stat(stat: &str) -> Option<u32> {
    let (_, fields) = stat.rsplit_once(')')?;
    fields.split_whitespace().nth(1)?.parse().ok()
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
    const WEB_CONTENT: &[u8] = b"com.apple.WebKit.WebContent";

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
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe {
            (proc_pid_rusage(pid, RUSAGE_INFO_V0, usage.as_mut_ptr().cast()) == 0)
                .then(|| usage.assume_init().footprint)
        }
    }

    pub(super) fn is_webkit(pid: i32) -> bool {
        named(pid, WEBKIT)
    }

    fn named(pid: i32, start: &[u8]) -> bool {
        let mut name = [0_u8; NAME_BYTES];
        // Safety: the buffer has the size given, and the call writes no more.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let length = unsafe { proc_name(pid, name.as_mut_ptr().cast(), u32::try_from(NAME_BYTES).unwrap_or(0)) };
        usize::try_from(length).is_ok_and(|length| name.get(..length).is_some_and(|name| name.starts_with(start)))
    }

    /// WebKit's web content processes among those `used` adds up.
    pub fn page_processes() -> Vec<u32> {
        let own = process::id();
        let counted = counted().unwrap_or_default();
        counted
            .into_iter()
            .filter(|pid| u32::try_from(*pid).is_ok_and(|pid| pid != own) && named(*pid, WEB_CONTENT))
            .filter_map(|pid| u32::try_from(pid).ok())
            .collect()
    }

    /// The physical footprint, which is what Activity Monitor calls Memory, of this program and of
    /// WebKit's processes that belong to it. The system ties those to whoever is responsible for the
    /// program, which is the program itself from the Dock, and the terminal when started from one,
    /// so the name is checked too: a terminal may be responsible for much more.
    pub fn used() -> Option<u64> {
        Some(counted()?.into_iter().filter_map(footprint).sum())
    }

    /// `used`, with this process's own part and how many processes it adds up.
    pub fn parts() -> Option<(u64, u64, usize)> {
        let own = i32::try_from(process::id()).ok()?;
        let measured: Vec<(i32, u64)> =
            counted()?.into_iter().filter_map(|pid| footprint(pid).map(|bytes| (pid, bytes))).collect();
        super::added_up(own, &measured)
    }

    /// The processes `used` adds up: this one, and WebKit's that it is responsible for.
    pub(super) fn counted() -> Option<Vec<i32>> {
        let own = i32::try_from(process::id()).ok()?;
        // Safety: takes a number and only reads system state.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let responsible = |pid: i32| unsafe { responsibility_get_pid_responsible_for_pid(pid) };
        let ours = responsible(own);
        let mut pids = vec![0_i32; MOST_PROCESSES];
        let size = i32::try_from(pids.len() * size_of::<i32>()).ok()?;
        // Safety: the buffer holds `size` bytes, and the call writes no more.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let count = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), size) };
        pids.truncate(usize::try_from(count).ok()?);
        Some(pids.into_iter().filter(|pid| *pid == own || (responsible(*pid) == ours && is_webkit(*pid))).collect())
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

    /// Only the program is counted here, so it is all of the figure.
    pub fn parts() -> Option<(u64, u64, usize)> {
        let used = used()?;
        Some((used, used, 1))
    }

    /// `/proc/<pid>/comm` keeps 15 bytes of a name.
    const WEB_PROCESS: &str = "WebKitWebProces";

    /// WebKitGTK's web processes under the program, however deep.
    pub fn page_processes() -> Vec<u32> {
        let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
        let processes: Vec<(u32, u32)> = entries
            .flatten()
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| {
                let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
                Some((pid, super::parent_in_stat(&stat)?))
            })
            .collect();
        super::descendants(std::process::id(), &processes)
            .into_iter()
            .filter(|pid| {
                std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|name| name.starts_with(WEB_PROCESS))
            })
            .collect()
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
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entry =
            PROCESSENTRY32W { dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(0), ..Default::default() };
        let mut found = Vec::new();
        // Safety: the entry is as large as `dwSize` says, which the call checks.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let mut more = unsafe { Process32FirstW(snapshot, &raw mut entry) } != 0;
        while more {
            let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(entry.szExeFile.get(..length).unwrap_or_default());
            found.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
            // Safety: as above.
            // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
            more = unsafe { Process32NextW(snapshot, &raw mut entry) } != 0;
        }
        // Safety: the handle came from the snapshot above and is closed once.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe { CloseHandle(snapshot) };
        found
    }

    /// The private working set, which is what Task Manager calls Memory.
    pub(super) fn private_working_set(pid: u32) -> Option<u64> {
        let size = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS_EX2>()).ok()?;
        // Safety: takes a right and a number; the handle is closed below.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut counters = PROCESS_MEMORY_COUNTERS_EX2 { cb: size, ..Default::default() };
        let buffer = (&raw mut counters).cast::<PROCESS_MEMORY_COUNTERS>();
        // Safety: the buffer is as large as `size`, which the call is told and does not exceed.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        let read = unsafe { K32GetProcessMemoryInfo(process, buffer, size) } != 0;
        // Safety: the handle came from `OpenProcess` above and is closed once.
        // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
        unsafe { CloseHandle(process) };
        if read { u64::try_from(counters.PrivateWorkingSetSize).ok() } else { None }
    }

    pub fn used() -> Option<u64> {
        let own = process::id();
        let pids = super::counted(own, &processes(), |name| name.eq_ignore_ascii_case(WEBVIEW));
        let total: u64 = pids.into_iter().filter_map(private_working_set).sum();
        (total > 0).then_some(total)
    }

    /// WebView2's browser processes, right under the program: ending one takes its renderers and helpers with it.
    pub fn page_processes() -> Vec<u32> {
        let own = process::id();
        processes()
            .into_iter()
            .filter(|(_, parent, name)| *parent == own && name.eq_ignore_ascii_case(WEBVIEW))
            .map(|(pid, _, _)| pid)
            .collect()
    }

    /// `used`, with this process's own part and how many processes it adds up.
    pub fn parts() -> Option<(u64, u64, usize)> {
        let own = process::id();
        let measured: Vec<(u32, u64)> = super::counted(own, &processes(), |name| name.eq_ignore_ascii_case(WEBVIEW))
            .into_iter()
            .filter_map(|pid| private_working_set(pid).map(|bytes| (pid, bytes)))
            .collect();
        super::added_up(own, &measured)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod system {
    pub const fn used() -> Option<u64> {
        None
    }

    pub const fn parts() -> Option<(u64, u64, usize)> {
        None
    }

    pub const fn page_processes() -> Vec<u32> {
        Vec::new()
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
