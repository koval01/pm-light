# Architecture

How the app is built and why. The client is unofficial and not affiliated with Proton AG.

## Decisions

- macOS and Windows must work fully. Linux is wanted, and may lag.
- The window is drawn by [Slint](https://slint.dev), so it looks the same on every system and needs one codebase.
  It is used under the GNU GPL, version 3, which the app's own AGPL-3.0 may be combined with (section 13 of both).
- The app is under the AGPL-3.0 with additional terms in `NOTICE`. The About page shows the license, the author, the
  repository and every component with its version and license, read from the repository at build time.
- Everything is Rust. There is no JavaScript or TypeScript in the project and no Node in the build.
- A system webview draws only what is HTML: an HTML mail, the composer's editor, an attachment preview, a printout and
  Proton's verification page. Each is a child webview of its own, and the program hands each the few lines of script
  it needs. A mail has scripts off; nothing in any of them reaches the program's state.
- An HTML mail's webview starts with the list and stays, hidden and emptied between mails: WebKit takes about a second
  to start. A text mail never needs it.

## Layout

```text
core/      proton-core: everything that is not a window. HTTP, sign-in, keys, mail, MIME, HTML cleaning,
           composing, sending, the local store, the journal, printing, seals.
app/       proton-mail-light: the window, tray, menu, Keychain, sync, notifications, files, links, app lock.
app/ui/    Slint screens, icons, the logo, the dust.
app/lang/  translations of the interface, as gettext files.
vendor/    patched copies of softbuffer and i-slint-backend-winit, see their PATCHED.md.
scripts/   bundling and signing on macOS, the logo, hostile-input runs in Docker.
docs/      this, the protocol, security, and what has not been seen working yet.
```

| What the app needs             | Where it comes from                                    |
| ------------------------------ | ------------------------------------------------------ |
| Window                         | Slint with winit, software renderer                    |
| Webviews                       | `wry`, as child views of the window                    |
| Tray and menus                 | `tray-icon` and `muda`, one `muda` in the program      |
| Notifications, autostart       | `notify-rust`, `auto-launch` with `--hidden`           |
| `mailto:` and the Dock         | `incoming.rs`, by AppKit itself on macOS               |
| One copy at a time             | `single_instance.rs`, a local socket                   |
| Window size and place          | `window_place.rs`, in the preferences                  |
| Process protection             | `protection.rs`                                        |
| Installers                     | A macOS bundle by `scripts/bundle-macos.sh`; no others |

## Words

`app/ui/say.slint` holds every text of the interface in English, by key, and Rust asks for text through it. The other
languages are `app/lang/{ru,de}/LC_MESSAGES/proton-mail-light.po`, which Slint compiles in. A new key goes into
`say.slint` and both files. `app/tests/words.rs` fails on a text with no translation, an empty one, placeholders that
differ, and a key the code asks for that does not exist. `app/tests/english_only.rs` keeps every other file English.

## Measured on macOS

Footprint in MB, with the real core and the real mailbox.

|                               | App | WebKit | Total               |
| ----------------------------- | --- | ------ | ------------------- |
| Native, list with WebKit warm | 74  | 19     | 93                  |
| Native, an HTML mail open     | 69  | 36     | 105                 |

These are from the build that keeps WebKit running. The earlier, smaller figures flickered under load. The print panel
loads the system's printing code once, about 30 MB, and that stays until the app quits. A hidden window still holds its
frame, 54 MB in all.

What holds them down:

- **Stepping through mail** opens only where the keys stop: `step_to` in `view.rs`.
- **A steady window comes first.** `keep_to_drawing` in `mail_view.rs` turns off what a mail does not need and keeps
  nothing on disk, and drawing stays on WebKit's GPU path.
- **A mail opens from what is at hand.** It goes on screen first, whole and already fitted. Its logo and the check of
  its pictures follow. The mails on either side are read ahead, and everything is kept sealed on disk: the mail, its
  files, its pictures and what Proton said about them.
- **The window's frames.** Two surfaces are taken in turn and only what changed is drawn: `softbuffer` made a new one
  for every frame and repainted the window whole. See `vendor/softbuffer/PATCHED.md`.
- **Memory is returned every 10 s**, from the program's heap and the system's. Steadiness comes first.

The footer shows the RAM the app takes as the system reports it, `phys_footprint`, with WebKit's processes added. On
Windows it shows nothing yet, and on Linux only the main process.

## Screens

Settings are a mode of the main window. A page is a list of rows that Rust puts together in `app/src/settings`, and
`app/ui/settings.slint` knows how to draw each kind, so a page costs nothing until its tab opens.

Hidden values sit under dust, like a spoiler in a chat. The loop is baked into tiles in `app/ui/dust` that join at their
edges, so a strip of any width is one tile repeated.

The logo is drawn from `app/ui/logo/source` by `scripts/logo.sh`, which also makes every app icon and the tray icon.

## Building and signing

`cargo build --profile native -p proton-mail-light` builds the program, and `scripts/bundle-macos.sh` puts it into
`target/native/Proton Mail Light.app` and signs it. macOS takes the menu bar's name and the Dock icon from the bundle, so
a bare binary shows its file name and no icon.

`scripts/sign-local.sh` signs with an Apple Development certificate when there is one, and for no one otherwise. The
Keychain admits a program by its Team ID, or by the exact hash of the build when it has none, so a self-signed
certificate does not help: "Always allow" holds only for that one build. With a free Apple Development certificate it
holds across builds: checked with a rebuild that changed the code hash.

`scripts/check.sh` comes first in every build: `cargo deny`, clippy with the strict rules of the root `Cargo.toml`, and
every test. `scripts/platforms.sh` runs the Linux tests and the Linux and Windows lints in Docker, which catches code
that compiles only on macOS. `.github/workflows/build.yml` runs the checks, builds on macOS, Windows and Linux, starts
the app on each, and keeps a picture of the screen 20 seconds later. `.github/workflows/codeql.yml` is GitHub's code
scanning. Both run only once the code is on GitHub.
