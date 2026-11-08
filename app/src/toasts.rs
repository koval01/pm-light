//! The notes at the bottom of the window, by number, which `app.slint` turns into words. Every
//! number lives here, so that two places can never mean different things by one, as a folder that
//! failed to save once said "Sender blocked". `tests/toasts.rs` holds the two sides together.

pub const FAILED: i32 = 1;
pub const BLOCKED: i32 = 2;
pub const SAVED: i32 = 3;
pub const SENT: i32 = 4;
pub const DRAFT: i32 = 5;
/// Shown by the window itself, when an address is copied.
#[expect(dead_code, reason = "app.slint sets it, and tests/toasts.rs reads it here")]
pub const ADDRESS_COPIED: i32 = 6;
pub const UNSUBSCRIBED: i32 = 7;
pub const RECEIPT_SENT: i32 = 8;
pub const INTERNAL: i32 = 9;
pub const RESTARTED: i32 = 10;
pub const SCHEDULED: i32 = 11;
pub const NO_ENGINE: i32 = 12;
pub const PICTURE_COPIED: i32 = 13;
/// A mail too long for one picture: it holds the beginning.
pub const PICTURE_CUT: i32 = 14;
