//! What a program can do for itself against the rest of the machine. It cannot stop an administrator
//! or the kernel, and a webview's memory belongs to the system.

/// Goes first: everything after this runs with the protections in place. The journal has not
/// started yet, so what did not take is handed back to be written there.
pub fn harden_process() -> Vec<&'static str> {
    #[cfg(not(target_os = "windows"))]
    let mut missing = Vec::new();
    #[cfg(unix)]
    {
        // A crash must not leave a file holding tokens and mail.
        let none = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
        // Safety: a limit of this process, from a structure alive for the call.
        if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &raw const none) } != 0 {
            missing.push("core files could not be turned off");
        }
    }
    // Debug builds stay debuggable.
    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    // Safety: a request about this process alone, with no address to read or write.
    if unsafe { libc::ptrace(libc::PT_DENY_ATTACH, 0, std::ptr::null_mut(), 0) } != 0 {
        missing.push("debuggers could not be kept out");
    }
    // A build for AddressSanitizer leaves the process readable: its leak check reads it from outside.
    #[cfg(all(target_os = "linux", not(checked_memory)))]
    // Safety: a setting of this process, with plain numbers.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        missing.push("the process could not be made unreadable to others");
    }
    #[cfg(target_os = "windows")]
    return windows::harden();
    #[cfg(not(target_os = "windows"))]
    missing
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::c_void;
    use windows_sys::Win32::System::{
        ErrorReporting::{WER_FAULT_REPORTING_FLAG_NOHEAP, WerSetFlags},
        SystemServices::PROCESS_MITIGATION_IMAGE_LOAD_POLICY,
        Threading::{ProcessImageLoadPolicy, SetProcessMitigationPolicy},
    };

    /// No libraries from a network share or from a low-integrity folder, and system ones first.
    const NO_REMOTE_IMAGES: u32 = 1;
    const NO_LOW_LABEL_IMAGES: u32 = 1 << 1;
    const PREFER_SYSTEM32_IMAGES: u32 = 1 << 2;

    pub fn harden() -> Vec<&'static str> {
        let mut missing = Vec::new();
        // A crash report goes to Microsoft or a local dump with the heap left out: that is where
        // tokens and mail would be.
        // Safety: a plain call with a defined flag.
        if unsafe { WerSetFlags(WER_FAULT_REPORTING_FLAG_NOHEAP) } < 0 {
            missing.push("crash reports could not be kept from the heap");
        }
        let mut policy = PROCESS_MITIGATION_IMAGE_LOAD_POLICY::default();
        policy.Anonymous.Flags = NO_REMOTE_IMAGES | NO_LOW_LABEL_IMAGES | PREFER_SYSTEM32_IMAGES;
        let size = size_of::<PROCESS_MITIGATION_IMAGE_LOAD_POLICY>();
        // Safety: the buffer is the policy's own structure, of its own size, alive for the call.
        let set = unsafe { SetProcessMitigationPolicy(ProcessImageLoadPolicy, (&raw const policy).cast::<c_void>(), size) };
        if set == 0 {
            missing.push("the library loading policy was not taken");
        }
        missing
    }
}

/// macOS naps hidden apps and stretches their timers by minutes, which would delay notifications.
#[cfg(target_os = "macos")]
pub fn keep_awake_in_background() {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    let reason = NSString::from_str("Checking for new mail");
    let activity = NSProcessInfo::processInfo()
        .beginActivityWithOptions_reason(NSActivityOptions::UserInitiatedAllowingIdleSystemSleep, &reason);
    // Held for the life of the process. The machine itself may still go to sleep.
    std::mem::forget(activity);
}

#[cfg(not(target_os = "macos"))]
pub const fn keep_awake_in_background() {}
