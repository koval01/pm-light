# Not seen working yet

Everything from the old interface on `main` has a place in the native window: the file-by-file ledger of that port is
in the history before `543459f`. What follows has been written and tested, but not yet seen in a real run with a real
account, or is known to differ. A line goes only after a run of the real build. The client is unofficial and not
affiliated with Proton AG.

## Not yet pressed on a real account

- **Sign-in:** the app lock (setting, opening and freezing), the waiting screen on a real outage, Proton's check opening
  on the first try.
- **Writing:** sending of every kind except one external address and password mail, which were seen working:
  plain text, scheduled, with expiry, with undo, from the options panel and the send button's menu.
- **List:** the bulk bar's actions, Empty folder, star on real mail; pages after mail was moved off the last one.
- **Reader:** the receipt banner, unsubscribing, the toolbar's actions on real mail, the print dialog and seals printed
  for real.
- **Contacts:** adding and removing.
- **Palette and keys:** the commands with a real account.
- **Settings:** most pages were seen on a free test account. Account, security, sessions, keys, domains,
  recovery, subscription, forwarding, the app's own pages and the print check have not been used with a real account.
  Pictures of seal sheets are not shown.
- **System:** notifications, the tray's menu, autostart after a real login, Touch ID.
- **A mail as a picture:** on Windows (DevTools' `captureBeyondViewport` on a webview out of sight, and the clipboard)
  not yet seen: CI draws a made-up mail with `--check-picture`. On Linux it was drawn (720 by 1929) and copied under a
  virtual screen in Docker, by `scripts/platforms.sh`; mail itself has not been opened there. Sharing exists only on
  macOS: Windows' own share sheet is not used yet. On macOS, pasting into Telegram and WhatsApp has not been tried.

## Account settings not followed yet

Saved to the account for Proton's own apps, and the settings say so. Everything else on those pages acts here.

- A folder taking its parent's colour: folders here are flat.
- Signing for external recipients, attaching the public key, the PGP scheme and the prompt to trust keys.

## Conversations

With `ViewMode` 0 a row is a conversation: its id is the conversation's, `messages` says how many it holds, and every action
goes to `/mail/v4/conversations/*`, so it reaches the whole conversation as in Proton's client. Search and Drafts and Sent
stay single messages. `message_of_row` picks the mail that opens, by `findMessageToExpand`. The toolbar acts on the row
(`current_id`), and whatever needs a real message takes `open_message_id`. Star, unread, moving and counts were seen live
with a draft reply as the second mail; a conversation with real mail from several people has not been.

## Attachment viewer

Pictures, video, sound, PDF and text are shown, chosen by bytes in `core/src/file_kind.rs`. Formats are what the system's engine
plays: MP4, MOV and WebM with its codecs, not MKV or AVI. Office files, archives and the rest have an icon and are saved. Only
pictures have thumbnails; a video's or a PDF's first page would need a decoder here or a file on disk.

## Image metadata

Pictures are cut, not encoded again as in Proton's client, so their pixels stay as they are. A JPEG keeps only what drawing
needs, with a one-entry `Exif` block when the photo is turned, and anything after its end goes. PNG and WebP keep a fixed
list of chunks. Files attached from disk go through this; a picture pasted into the editor and files carried over in a
forward do not.

## Embedded pictures

Pictures in `<img>` are held back on the page alone, and the stored mail keeps them. A background picture in a style is not
held. Showing remote and embedded ones is one button, so an account that shows remote pictures but hides embedded ones gets
both only when asked. Mail kept on disk before `from_proton` existed is read again once, if the account hides pictures.
Pressing Show images on held embedded pictures has not been seen live.

## Plain text letters

A new letter follows `DraftMIMEType`, and any other takes the type of the mail it starts from, as in Proton's client: a
reply to HTML mail stays HTML whatever the setting says. Saved drafts were seen coming back. A real `text/plain` send is
left to a real run.

## To check before the first Windows run

- Prepared before any run: every webview has a data folder of the app's own, a mail and the editor come over a protocol
  instead of `NavigateToString` and its 2 MB, notifications carry a registered app id, the title bar follows the theme,
  crash reports leave out the heap, and a webview that cannot start says so. None of it has run on Windows yet.
- A `mailto:` link starts a second program on Windows and Linux. `single_instance.rs` hands it to the first one over a
  local socket, which was seen working on macOS only. Windows does not yet know that the app takes `mailto:`: an
  installer has to register it.
- The start without a console window (`windows_subsystem`), the icon in the program file, the tray, notifications and
  the Credential Manager have not been seen.
- A panel over an open mail (the account panel, the tour) shows a picture of the mail's page on macOS, taken by
  WebKit. Windows and Linux have no picture yet: there the page is hidden while the panel is open.
- The memory figure in the footer adds the program's private working set to its WebView2 processes', as Task Manager
  shows them. Only the choice of processes is tested; the figure itself has not been seen on Windows.

## Notifications on macOS

The bundle asks the system for notifications with badges at its start, and the Dock shows the unread count only once
they are allowed: once the system answered "not allowed" without asking, because an answer was already
recorded for the app, and the count appeared after the switch was turned on in the system's settings. The question a
new person gets at the first start has not been seen, nor the notice in Settings, under Application, that points to
the system's settings when they are refused.

## Seen once, not reproduced

- The bundle on macOS: an exception inside the system's print panel (`PrintingUI`) while it laid
  out its column of page pictures, during an animation, which ended the app. Afterwards, printing a mail through
  that panel five times, cancelling it at once or after scrolling the page pictures, did not bring it back, and nor
  did asking the app to quit while the panel was open: the system held the quit back until the panel was cancelled,
  and then dropped it. What was done in the panel that time is not known.

## Tor

Checked: arti reaching Proton through the onion site and through an exit, with the circuit's countries, and the door
refusing other hosts (`live_tor_` test). Seen in the window on macOS: the first question, the
Tor tab with the circuit drawn, the sidebar line, new circuits, the start waiting for Tor, and the waiting screen with
its way out of Tor (with the network blocked). Not yet seen: a relaunch after changing the route, Proton's
verification page through Tor, and anything of Tor on Windows. Not done: bridges for networks that block Tor.

## Known gaps on Linux

- The menus that open from the window (More, labels, snooze, the sender list) are system menus, and `muda` can show
  one only over a GTK window. The window is winit's, so on Linux these menus do not open at all. They need a menu drawn
  by Slint there.

## Not looked at yet

- A link arriving while the app is locked, which is written to wait for the lock to open.
- Marking spam with unsubscribing on, and blocking with the question turned off.
