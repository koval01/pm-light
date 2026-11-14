# Proton Mail Light

[![Build](https://github.com/koval01/pm-light/actions/workflows/build.yml/badge.svg?branch=main)](https://github.com/koval01/pm-light/actions/workflows/build.yml)
[![Security](https://github.com/koval01/pm-light/actions/workflows/security.yml/badge.svg?branch=main)](https://github.com/koval01/pm-light/actions/workflows/security.yml)
[![CodeQL](https://github.com/koval01/pm-light/actions/workflows/codeql.yml/badge.svg?branch=main)](https://github.com/koval01/pm-light/actions/workflows/codeql.yml)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/koval01/pm-light/badge)](https://scorecard.dev/viewer/?uri=github.com/koval01/pm-light)
[![OpenSSF Best Practices](https://www.bestpractices.dev/projects/16464/badge)](https://www.bestpractices.dev/projects/16464)
[![Release](https://img.shields.io/github/v/release/koval01/pm-light?include_prereleases&sort=semver)](https://github.com/koval01/pm-light/releases)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue)](LICENSE)
[![Rust 1.100](https://img.shields.io/badge/rust-1.100-orange?logo=rust)](rust-toolchain.toml)
![macOS | Windows | Linux](https://img.shields.io/badge/systems-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)
![Unofficial](https://img.shields.io/badge/Proton-unofficial%20client-555)

Unofficial lightweight Proton Mail desktop client, written in Rust from end to end. The window is drawn by
[Slint](https://slint.dev); a system webview shows only what is HTML.

> **This is an independent project. It is not an official Proton client, is not affiliated with Proton AG,
> and is neither endorsed nor supported by that company.** Proton and Proton Mail are trademarks of Proton AG.

**Author:** Yaroslav Koval ([@koval01](https://github.com/koval01))

## What this client is

- It talks to the Proton API **directly**, without Proton Bridge, and decrypts mail locally on the device
  (SRP sign-in, keys, PGP). The password goes nowhere but Proton.
- It introduces itself honestly: `x-pm-appversion: Other` and a `User-Agent` with the client's name and author
  (see [core/src/client.rs](core/src/client.rs)). It **does not pose** as an official Proton app.
- It does not solve human verification (CAPTCHA). Proton's full page opens in a native webview inside the app's
  window, a person passes it, and sign-in carries on by itself. The same check can also be opened in the browser.

## Important

This is Proton's private, undocumented API. It can change at any time, and using a third-party client may break
Proton's terms and lead to temporary account restrictions (code 2028). Use it only on accounts you own, preferably
a test one.

## Running

Building takes [Rust](https://rustup.rs); the version is pinned in `rust-toolchain.toml` and installs itself. On Linux
the WebKitGTK and GTK development packages are needed too (see `.github/workflows/build.yml`).

```bash
cargo run -p proton-mail-light                       # the app, built for debugging
cargo build --profile native -p proton-mail-light    # the app as it is released, in target/native
scripts/bundle-macos.sh                              # then the macOS bundle, in target/native
cargo test --workspace                               # every test
cargo clippy --workspace --all-targets
scripts/attacks.sh                                   # hostile input, in Docker only
```

The window looks the same everywhere. Mail is drawn by WebKit on macOS, WebView2 on Windows and WebKitGTK on Linux.
Only macOS has been used for real so far. CI builds and starts the app on Windows and Linux too, and draws a made-up
mail on each.

On Linux the program needs WebKitGTK 4.1, GTK 3 and the X11 keyboard library at run time; on Ubuntu and Debian:
`libwebkit2gtk-4.1-0 libgtk-3-0 libxkbcommon-x11-0`. Without the last one the window cannot start.

## Layout

- `core` (`proton-core`): everything that is not a window. HTTP, sign-in, keys, mail, MIME, HTML cleaning,
  composing, sending, the local store. Cryptography: Proton's official crates (`proton-srp`, `proton-rpgp`,
  `proton-crypto-account`).
- `app` (`proton-mail-light`): the window, tray, menu, Keychain, sync, notifications, files, links, `mailto:`, app lock,
  process protection. Its screens are in `app/ui`, and translations in `app/lang`.

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) explains the choices, and [docs/UNCHECKED.md](docs/UNCHECKED.md) lists what
has not been seen working yet.

## Features

Sign-in (SRP, 2FA, human verification), session in the system Keychain, all account addresses with a sender choice,
names and signatures, folders and labels, counters, search, threads, reading, attachments, composing, reply, reply
all, forward, draft autosave, undo send and scheduled send, bulk actions, attachment preview, message headers,
new-mail notifications, formatting editor, auto-reply, filters with a Sieve editor, blocking senders, unsubscribing
from mailing lists, "Not spam", password-protected and expiring mail, read receipts, snoozed mail, contacts, active
sessions, plan, sign-in log, message details, printing, a report of what was blocked, contact autocomplete, spam,
trash, permanent deletion, settings, full keyboard control (`Ctrl/⌘+K` opens every command, `?` shows the keys).
Light and dark themes; English, Russian and German.

New mail appears by itself: a Rust thread compares folder counters every 30 seconds, also while the window is hidden.

Native parts: tray, system menu, system notifications, background sync, single instance, `mailto:` links, autostart,
remembered window size and place, wake detection, attachment previews of pictures, video, sound, PDF and text, chosen by
the file's bytes.

Not done: PGP for external recipients, images inside a composed message, calendar, Drive.

## Security

Mail, attachments and links are treated as hostile input. [docs/SECURITY.md](docs/SECURITY.md) has the threats, the
audit and the known limits.

- **HTML.** Rust cleans every message with `ammonia`, then `lol_html`; CSS goes through Servo's `cssparser`. Size and
  nesting are limited, and a message past the limits is shown as plain text. The result is shown in a webview of its
  own with scripts off and a policy that gives it no network.
- **Images.** Remote images stay held back until asked for, then come through Proton's proxy (`core/v4/images`), so
  the sender does not see your address.
- **Attachments.** The real type is detected with `infer` and image dimensions with `imagesize`. Runnable files get a
  warning, and saved files are marked as downloaded (quarantine on macOS, Zone.Identifier on Windows).
- **Links.** Only web and mail addresses leave the app, and tracking parameters are removed.
- **Connection.** Only Proton's own keys are accepted, as its Bridge pins them, and redirects are refused.
- **Session.** A session revoked elsewhere ends locally at once. Losing the network pauses sync; `/tests/ping`
  notices when it returns.
- **Local data.** Read mail is kept on disk for the next start, each file sealed on its own, and signing out erases it.
  The session is stored in the system Keychain, and keys come only from the server. With an app password the session is sealed with Argon2id and XChaCha20-Poly1305.
- **App lock.** An app password, plus Touch ID on macOS. Every wrong password costs a pause, and from the fifth one
  the password is frozen for 30 seconds, growing to an hour; a restart does not lift this. Touch ID is a software gate over a Keychain item, not bound
  to hardware, because an ad-hoc signed build cannot use a biometric access control.
- **Memory.** Secrets are zeroed on drop, core dumps are off, and debuggers are refused where the system allows it.
  This does not stop an administrator or the kernel, and a webview's heap cannot be wiped.

## Matching Proton

The protocol is checked against `go-proton-api` and the official web client: see [docs/PROTOCOL.md](docs/PROTOCOL.md)
and [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## License

Copyright (C) 2026 Yaroslav Koval. Free software under the [GNU Affero General Public License, version 3](LICENSE),
with additional terms under its section 7, written out in [NOTICE](NOTICE):

- every copy and every changed version keeps the author's name and the link to https://github.com/koval01/pm-light,
  in the source and on its About page;
- a changed version says that it is changed, by whom, and where its source is;
- the terms give no right to the names "Proton Mail Light" or "Proton".

Anyone who passes on the program, changed or not, or runs a changed version for others, gives its source under the
same license and terms. It comes with no warranty.

The program is built from open-source components. `scripts/components.py` lists them with their versions and
licenses in `app/components/`, with the texts of their licenses; the About page shows the list, and the texts come
with the program. `app/tests/components.rs` fails when the list no longer matches `Cargo.lock`.
