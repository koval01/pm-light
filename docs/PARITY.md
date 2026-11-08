# Against Proton's web client

What Proton's own web client offers that this client does not yet, as the author saw it there. Each
item is checked against Proton's sources before it is built (`docs/PROTOCOL.md` records the calls), and leaves this
list when it is done. Proton's paid writing assistant is left out: it runs on Proton's servers or a downloaded model,
and is not a mail client's part.

## The list

- Sort a folder: newest or oldest first, by size growing or shrinking; a "read" filter beside "unread" and "has
  attachments".
- Below the folders: the space used out of the space there is, as a bar.

## A mail

- A shield beside the date with the number of trackers blocked; pointing at it says how many trackers and cleaned
  links, and a click lists them by company with their addresses. The app has this list under More, unmarked.
- "Filter by" from a mail: subject, sender, recipient, attachment, leading to a new filter.
- Moving and labelling from a mail: a list with a search, and "always do this for mail from this sender".
- Sending a read receipt when the sender asked for one.
- Reporting phishing, from More.
- Exporting, and showing the mail as HTML, from More.
- Going to the newer or older mail from the reader.

## Writing

- Sending later from the arrow beside Send: tomorrow at 8, Monday at 8, or a time chosen.
- Choosing the address it is sent from, with a way to the address settings.
- A mail locked with a password for addresses outside Proton, with a hint, expiring after 28 days.
- More: plain text, attaching the public key, asking for a read receipt, and how long the mail lives.

## Settings

- Quick settings from the gear: layout, density, the writing window, keyboard shortcuts, colour scheme with a light
  and a dark theme to follow the system.
- Making the app the system's mail program.

## Beyond Proton

- Packing many attachments into one zip before sending: offered when several files are attached, done by the app.
- Looking into an attached archive without unpacking it to disk, after strict checks: the unpacked size and the ratio
  of packing, the number of entries and of archives inside archives, names that climb out of the archive or are
  absolute, all bounded, reading headers first and stopping a stream at its limit. Hostile archives are tested in
  the attack run only, with crafted ones.

## Done

- Pictures from Gravatar for senders with no logo, off unless turned on after a warning, through Proton's image proxy.
- A picture of the whole mail, its sender, subject and date over the body, copied to the clipboard to paste into a
  chat, or handed to the system's menu for sharing on macOS.
- A folder a page at a time, as Proton shows it, with "‹ 3/8 ›" beside the folder's name and a menu of the pages; one
  long list instead, chosen at the first start or in Appearance.
- Days left before the trash and spam delete a mail, in the list and in the open mail, with the notice in those
  folders; a close button on the open mail; hints on every icon button of the window.
