//! The counts the window shows: each folder's unread mail, the inbox's in the Dock and the tray, and each category's
//! on its tab. They all come from one answer of Proton's, and are drawn together from it.

use crate::{App, categories};
use proton_core::model::Counts;
use slint::Model;

/// What the heading above the list says is about the folder open, whichever that is.
pub fn show_folder_unread(app: &App) {
    let open = app.get_folders().iter().find(|folder| folder.active);
    app.set_folder_unread(open.map_or(0, |folder| folder.unread));
}

/// The mail not yet read in the inbox, in the one place it is shown from: the window, the Dock and the tray together.
/// Two ways set it, the folders at the start and the list after it, and the badge once followed only a third.
fn show_inbox_unread(app: &App, unread: u64) {
    app.set_inbox_unread(i32::try_from(unread).unwrap_or(i32::MAX));
    crate::tray::show_unread(app, unread);
}

/// Every count the window shows, and the Dock and the tray with them: a count taken by the list itself, after a mail
/// was read, leaves the background check nothing new to report, and the badge kept the old number.
pub fn show(app: &App, counts: &Counts) {
    let unread = |id: &str| counts.get(id).map_or(0, |count| i32::try_from(count.unread).unwrap_or(i32::MAX));
    show_inbox_unread(app, counts.get("inbox").map_or(0, |count| count.unread));
    let folders = app.get_folders();
    for index in 0..folders.row_count() {
        if let Some(mut folder) = folders.row_data(index) {
            let now = unread(folder.id.as_str());
            if folder.unread != now {
                folder.unread = now;
                folders.set_row_data(index, folder);
            }
        }
    }
    show_folder_unread(app);
    categories::show_counts(app, counts);
}
