//! Whom a process runs as on Windows: the security identifier of its token's user, as bytes to compare.

use std::ptr::null_mut;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{GetLengthSid, GetTokenInformation, IsValidSid, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
};

/// The person this program runs as.
pub fn own() -> Option<Vec<u8>> {
    // Safety: takes nothing, and gives a handle that stands for this process and is never closed.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    user_of(unsafe { GetCurrentProcess() })
}

/// The person another process runs as. Nothing when it cannot be asked, as another account's process mostly cannot.
pub fn of_process(pid: u32) -> Option<Vec<u8>> {
    // Safety: takes a right and a number, and gives a handle that is closed below.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let person = user_of(process);
    // Safety: the handle came from `OpenProcess` above and is closed once.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe { CloseHandle(process) };
    person
}

fn user_of(process: HANDLE) -> Option<Vec<u8>> {
    let mut token: HANDLE = null_mut();
    // Safety: the process's handle is open, and the token's is written to a local that outlives the call.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) } == 0 {
        return None;
    }
    let person = user_of_token(token);
    // Safety: the handle came from `OpenProcessToken` above and is closed once.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe { CloseHandle(token) };
    person
}

fn user_of_token(token: HANDLE) -> Option<Vec<u8>> {
    let mut needed = 0_u32;
    // Safety: no buffer and a length of zero ask only for the size, which is written to a local.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &raw mut needed) };
    // In words of eight bytes, so that the structure at its start is aligned as it must be.
    let words = usize::try_from(needed).ok()?.div_ceil(size_of::<u64>());
    if words * size_of::<u64>() < size_of::<TOKEN_USER>() {
        return None;
    }
    let mut buffer = vec![0_u64; words];
    let size = u32::try_from(words * size_of::<u64>()).ok()?;
    // Safety: the buffer holds `size` bytes, which the call is told and does not exceed.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    if unsafe { GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), size, &raw mut needed) } == 0 {
        return None;
    }
    // Safety: the call wrote a `TOKEN_USER` at the start of the buffer, which is large enough and aligned for one.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let sid = unsafe { buffer.as_ptr().cast::<TOKEN_USER>().read() }.User.Sid;
    // Safety: the identifier points into the buffer, which lives to the end of this function.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    if unsafe { IsValidSid(sid) } == 0 {
        return None;
    }
    // Safety: as above, and the identifier was found valid.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    let length = usize::try_from(unsafe { GetLengthSid(sid) }).ok()?;
    // Safety: a valid identifier is `length` bytes long, inside the buffer the call filled.
    // nosemgrep: rust.lang.security.unsafe-usage.unsafe-usage
    Some(unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), length) }.to_vec())
}
