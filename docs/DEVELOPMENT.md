# Developing Proton Mail Light

How the project is laid out, built and checked, and what it has learned the hard way.
[`CONTRIBUTING.md`](../CONTRIBUTING.md) is the short version for a first change.

## Language

Write anything in this project only in English: documentation, comments, error messages, logs, test text, commit
messages. Other languages exist only in the translation files (`app/lang`). `app/tests/english_only.rs` enforces this.

## Layout

- `core` (`proton-core`): everything that is not a window. HTTP, sign-in, keys, mail, MIME, HTML cleaning, composing,
  sending, the local store, the journal, printing, seals. The entry point is `service.rs`, with one file per area in
  `service/`. The mailbox lives in `view.rs`. Cryptography lives only in `crypto.rs`: key objects never leave that file.
- `app` (`proton-mail-light`): the window, tray, menu, Keychain, sync, notifications, files, links, `mailto:`, app lock,
  process protection (`protection.rs`), panic containment (`guard.rs`). Screens are in `app/ui`, preferences in
  `app/src/preferences.rs`.
- `vendor`: patched `softbuffer` and `i-slint-backend-winit`, each with a `PATCHED.md`.

## Systems

macOS and Windows are the targets, and Windows is supported in full. On Windows that means Windows 10 22H2 (build
19045, the last release of Windows 10) and Windows 11: nothing older is supported, so their APIs, and the WebView2
Runtime that comes with them, may be relied on. Linux is wanted, and may lag. Windows is checked by
`scripts/platforms.sh` and by CI.

## Only Rust

The product is built from machine code, and every line of it is Rust: never, anywhere and in no form, is the app
written in another language. Programs written in other languages may be used as dependencies and tools, and scripts
for side work around the product (checks, builds, bundling, attacks, drawing the logo) may be shell or Python. Nothing
the app itself runs may be: no JavaScript, TypeScript, Python or other interpreted code in it, and no Node or other
runtime in what ships.

Not yet true, and to be removed: the few lines of JavaScript the webviews get as strings from Rust (the editor's
reporting, fitting a mail to its pane, placing a printout, relaying Proton's check). None of them holds state, tokens,
keys or raw Proton responses.

A file holds one area. Tests live in `name/tests.rs` beside `name.rs`. When a file grows past a few hundred lines of
code, split it before adding to it. `docs/ARCHITECTURE.md` explains the choices.

## License

AGPL-3.0 (`LICENSE`) with additional terms under its section 7 (`NOTICE`): the author's name, the link to the
repository and a mark on changed versions stay. The About page shows them, read from those files at build time. After
any change to the dependencies run `scripts/components.py`: `app/tests/components.rs` fails until the list in
`app/components/` matches `Cargo.lock`. Every source file is under the same license.

## Words

`app/ui/say.slint` holds each interface text in English, by key. A new key goes into it and into both
`app/lang/{ru,de}/LC_MESSAGES/proton-mail-light.po`, braces doubled where Rust fills a value in. `app/tests/words.rs`
fails on a missing or empty translation, placeholders that differ, and a key the code asks for that does not exist.

## Journal and failures

- `journal.rs` records what the app does through the `log` macros. Each record is sealed with a key from the Keychain,
  files last an hour, nothing outlives a day, and quitting erases them with the webviews' caches. A crash skips that on
  purpose: the record is what explains it.
- Never pass a journal macro an address, a subject, a file name, a token, a server's message or mail text. Use numbers,
  `error.kind()` and `journal::clean_path`. Panic messages stay out for the same reason, only the place is kept.
- The journal is detailed: a start with the version and system, folders, mail opened with its timing and counts, the
  editor, sending, previews, settings tabs, failed actions by a fixed word. `app/tests/journal_privacy.rs` reads every
  record in both crates and fails on anything of the person's or an error's own text.
- Boundaries are few and central: `guard::contain` for threads and native callbacks. A panic on a worker ends only its
  task and frees the window. One on the window's thread restarts the program, at most three times in a row.

## Commands

```bash
scripts/check.sh                                     # dependencies, lints, tests: before every build
scripts/bundle-macos.sh                              # check, build and bundle in target/native, signed
cargo run -p proton-mail-light                       # the app, built for debugging
scripts/platforms.sh                                 # Linux tests and lints, Windows lints, in Docker
scripts/attacks.sh                                   # hostile input, in Docker only
```

`scripts/check.sh` runs `cargo deny` (`deny.toml`), clippy over everything with warnings as errors, and every test.
The lints are in the root `Cargo.toml`: clippy's pedantic and nursery groups whole, with four exceptions that say
why, and `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, indexing and string slicing that can fail, and
`process::exit` as errors outside tests. A cast goes through `app/src/numbers.rs` or `try_from`, never a bare `as`
that could lose a sign or digits. Never silence one to get a build through: an
exception carries `#[expect(..., reason = "...")]` at the one place it is needed, and only Slint's generated code is
fenced off whole. Before a change is pushed: `scripts/check.sh`, `scripts/platforms.sh`, and for anything in the
window, a look at the bundle running.

`.github/workflows/build.yml` does the same on macOS, Windows and Linux on every push to main and every pull request,
with the dependencies cached from main. There it starts the debug build the tests made; the release profile, a quarter
of an hour of whole-program optimisation, is built only for a release. It starts the app on each with `--check-picture`, which draws a made-up mail through the system's web engine, and
`--check-window`, which maximises the window, makes it full screen and brings it back while CI photographs each: read
the `window-*.png`, `picture.png` and the screens after a change that touches them. `security.yml` runs cargo-deny, gitleaks and zizmor,
`codeql.yml` GitHub's code scanning, `scorecard.yml` the OpenSSF Scorecard, and pull requests get a dependency review.
A release is a tag `v<version>` (`release.yml`). `main` is protected by `.github/rulesets/main.json`, applied with
`gh api -X POST repos/koval01/pm-light/rulesets --input .github/rulesets/main.json`: changes come through pull
requests that pass these checks, and only an admin may bypass it.

## Checking

- Tests are never too many. Test everything: the normal path, the edges, hostile input and failure, and prove every
  new check by breaking the code and watching it fail.
- A test with the window or the network stubbed out says nothing about the real app. Behaviour where the window meets
  the core is checked by running a real build.
- First prove the check catches the error: put the bug back and see it fail.
- Every change to the window is looked at at its smallest size, 900 by 580, and at full screen.
- `app/tests/screens.rs` draws the main screens headless, from the smallest window to 4K and at one to two pixels to
  a point, and compares them with `app/tests/screens/*.png` on macOS; on Windows and Linux it writes them to
  `target/screens/`, which CI keeps. A change
  meant to be seen is written with `UPDATE_SCREENS=1 cargo test -p proton-mail-light --test screens`, and the new
  pictures are looked at before they are committed. A failure leaves the picture and its difference in
  `target/screens/`. A new screen or mode gets a picture there.
- Mail and previews are drawn by WebKit on macOS, WebView2 on Windows and WebKitGTK on Linux.
- Do not name a cause until it is confirmed.

## Attacks

Hostile-input tests never run on the host. `scripts/attacks.sh` builds with network, then runs the tests marked
`#[ignore]` in a container with no network, a read-only file system, no capabilities, and memory, CPU and process
limits. `html/parsed_tests.rs` parses the cleaned mail with html5ever, twice, and reads its CSS with a CSS tokenizer.
Docker must be running. Only crafted input is used, never real malware. Tests named `live_` reach Proton and are
skipped there.

`fuzz/` fuzzes the code that reads what strangers send: MIME, HTML and CSS, links, attachments, pictures and pasted
codes. It is a workspace of its own and needs a nightly compiler: `cargo +nightly fuzz run <target>`. CI runs every
target for a minute on each push and for ten once a day (`.github/workflows/fuzz.yml`). A new parser of outside input
gets a target there.

Threats, findings and known limits are in `docs/SECURITY.md`: update it with every security change.

## The account check

`.github/workflows/live.yml` opens the test account on macOS, Windows and Linux: `live_check` reads it, and the app,
started with `--check-account=<file>`, opens its inbox and draws its first mail through the system's web engine. It
borrows the session the maintainer's Mac saved, handed over with `scripts/live-session.sh`, never refreshes the tokens,
and sends nothing. It runs by hand only, and each run waits for the maintainer's approval in the environment
`live-account`, which holds the session. Its log is public: only numbers and fixed words, and no artifacts.

## Test builds

A build pointed at a local server opens a real window, and someone may try to sign in to it. It must say `TEST BUILD` on
screen, and it never touches the real Keychain entries.

## Known pitfalls

- **Verification page.** Proton's `frame-ancestors` forbids an iframe, and that is respected. The page runs at the top of
  a native child webview (`verify.rs`) and reports only by navigating to `proton-mail-light-verify://`. Navigation
  checks also see frames on macOS.
- **A refused refresh.** Only 400/401/422 on `/auth/v4/refresh` end a session, and only when repeated (`client.rs`).
  Everything else is a `Link`: the session stays, and the window is told how the connection stands.
- **DNS.** `core/src/dns.rs` asks only Quad9, over DoH, with its addresses built in. Quad9 speaks HTTP/2 only. Never add
  a fallback to the system's resolver or to the usual trust store: the party attacking the connection could trigger it.
- **TLS pins.** `core/src/tls.rs` accepts only Proton's keys. "Cannot reach Proton" on a working network may mean they
  changed: compare with `TrustedAPIPins` in proton-bridge, and check with `cargo test -p proton-core live_ -- --ignored`.
- **Printing.** `core/src/print.rs` builds the document, and a webview out of sight paginates it, each mail inside two
  nested shadow roots so that its `:host` rule reaches nothing else. WebKit ignores `@page size`, repeats neither
  `position: fixed` nor table headers on later sheets, and draws 1 CSS mm as 1.07 mm on paper. Never put a CSS filter on
  the whole document: it turns the text into a blurred picture. It also cuts a grid row in two at the end of a sheet,
  whatever `break-inside` says, so seal codes go in whole sheets of six. Check any change by printing through WebKit to
  a PDF and measuring it. A check code knows nothing of the paper: never word a result as if it did.
- **Seals.** `core/src/seal.rs` writes standard OpenPGP clear signatures, so that any tool can check them. Two decoders
  read a sheet, whole and block by block, because a code among its neighbours can be missed. Spare codes are
  Reed-Solomon over the text's parts. A signed line must not end in a space: GnuPG refuses the signature while our own
  library accepts it. Any change here is checked with GnuPG, from text read back off a printed page. `scan.rs` reads a
  sheet at several scales. Never name a state or claim ICAO or BSI conformity.
- **A lock guard lives to the end of its statement.** `(self.view.lock().x, self.list_options(0))` took the same
  `parking_lot` lock twice and froze every start. Take values out in separate statements. `view_*` have tests in
  `service/tests.rs`, which fail by timeout.
- **Keychain prompts.** Every new unsigned build makes macOS ask again, and `restore` waits for that answer. That is the
  `session` stage on the splash. `Always allow` lasts for that build only, unless it is signed with an Apple
  Development certificate (`scripts/sign-local.sh`).
- **One `muda` in the program.** Slint links it on macOS whether or not a menu is used, and `tray-icon` must stay on the
  same version. Two versions register one Objective-C class, which corrupts memory: a crash in the release build, and no
  menu in any build. Slint's own default menu also has to be off (`with_default_menu_bar`), or it takes the bar back.
- **A window after `show`.** Slint realises it later, so a child webview or a sheet fails with `WindowHandleError`
  until `modal::after_show`. It also puts the window on screen after the first frame, which tears a sheet off its parent.
- **Editing keys in Slint fields.** Slint knows Copy and Paste by their letter, so they die on a Cyrillic layout. The macOS
  menu bar takes them and `menu::edit` hands them on, and `shortcuts.rs` does the same on Windows.
- **`rfd` without a parent.** On macOS the alert then comes from another process, and the main thread waits behind it.
- **WebKit reduced to drawing.** `keep_to_drawing` must set its switches on a configuration before the webview exists:
  changed on a live page they do nothing or leave it blank. Drawing stays on WebKit's GPU path. Turning that off saved
  20 MB and left a mail a white sheet with no error unless `AcceleratedDrawingEnabled` went too. Do not save memory at the
  cost of smoothness: a steady window matters more than a smaller number. It also turns the media process off, so
  video and sound in the viewer take a plain webview.
- **Window frames on macOS.** `vendor/softbuffer` shows `IOSurface`s directly. A row must be a multiple of 64 bytes: at
  any other width the window is empty and nothing reports it, and most widths are such widths. So a buffer can be longer
  than `width * height`, and `vendor/i-slint-backend-winit` takes the row from its length. After touching either, check
  odd window widths, and that a worked window matches a whole repaint pixel for pixel.
- **A size for an `if`.** An element behind `if` gets its preferred size, not its parent's, and is centred. A mode that
  covers the window needs its `x`, `y`, `width` and `height` set.
- **Rows on a short page.** A layout with room to spare stretches what is in it, so a settings page on sample data can
  look right while a short real one falls apart. Lists of rows use `alignment: start`, and a control has a fixed height.
- **A window held in a thread local.** Copy its handle out before acting on it. Text set in a field runs that window's
  callbacks at once, and they reach for the same `RefCell`: paste in the palette crashed the app this way.
- **A Keychain entry of another build.** Reading it stops the start behind a system question, and every other Keychain
  call waits on it. The journal has an entry of its own for this reason.
- **The search field.** A Slint caret takes the colour of the text, and the coloured pieces need that text transparent, so
  the field draws its own caret. Spaces in the pieces are no-break ones, or a trailing one is not measured.
- **The application delegate.** `winit` panics if the delegate is not its own, so `incoming.rs` adds the two calls it
  needs to `winit`'s class. Check any change with `open -a` on the bundle: with a link while it runs, with a link as the
  start, and with no link for the Dock's reopen.
- **One copy at a time.** `single_instance.rs` hands a second start's arguments to the first copy and quits. A restart
  after a panic skips that check, or it would hand itself to the copy that is dying.
- **A field's line height.** A `TextInput` sets its lines closer than `Text`s stacked in a layout, and the difference adds
  up over a script. A layer that must match a field measures ten lines of a hidden field, as `CodeArea` does.
- **Sheets over sheets.** `moment_picker` opens over a composer or the search builder as readily as over the main window:
  it is given the window that asks, and `modal::fitted` is only for the main one.
- **A viewer for attachments.** `preview.rs` serves one file over a protocol of its own, which seeking in a video needs
  (`Range`, status 206). On Windows the protocol is `http://attachment.preview`.
- **A row is not always a message.** In conversations `current_id` is a conversation's id, and `message()` cannot take it. What
  needs a message uses `open_message_id` or `message_of_row`. Where a conversation opens depends on what is unread, so it is
  asked before the row is marked read.
- **The composer's editor.** Anything entering it goes through `html::for_editor`. A `<textarea>` drops the first newline
  of its text, so a plain letter is written with one more.
- **Slint's compiler and the stack.** It walks the window's tree deep enough to overflow a 1 MB main thread, which is
  what Windows gives one: `build.rs` runs it on a thread with 64 MB. Reproduce on macOS with `ulimit -s 1024`.
- **Linux needs `libxkbcommon-x11`.** winit opens it at run time and panics without it, before any window: the app
  then restarts and dies again, with nothing on screen. `libxkbcommon-dev` does not bring it. `platforms.sh` starts the
  app under a virtual screen with the journal echoed, which showed it.
- **GTK on Linux.** Menus, the tray and webviews are GTK's, the window is winit's: `gtk_loop.rs` starts GTK before
  them and hands its events on every 16 ms. wry needs its `x11` feature for a webview in the window, or refuses every
  one. WebKitGTK's switch for page scripts also stops the app's calls into the page: `webviews::PageScriptsOff`.
  A hidden WebKitGTK page is never laid out, so the mail picture's page is shown past the window's edge.
- **A build script runs on the building machine.** `cfg(windows)` in `build.rs` asks about that machine, not the one the
  program is for: read `CARGO_CFG_TARGET_OS`.
- **WebView2 and `with_html`.** `NavigateToString` takes 2 MB at most. Pages that can grow (a mail, the editor) come from
  `webviews::OnePage`, a protocol of the webview's own, on every system. Such a webview must be incognito: on Linux a
  shared context registers a protocol once, and a second webview would be refused.
- **A webview's data folder.** Every webview is made through `webviews::page` or `webviews::proton`. Without a folder,
  WebView2 writes beside the program, which fails under Program Files.
- **Toasts are numbers.** Every number is named in `toasts.rs` and worded in `app.slint`; `tests/toasts.rs` holds them
  together. A folder that failed to save once said "Sender blocked".
- **Translations have a context.** Slint looks a text up by the component it is in. The same English in two components
  is two entries in each `.po` file. Every key of `say.slint` shares the context `Say`, so two keys with the same
  English share one translation: one with another meaning takes a context of its own, `@tr("key" => "text")`, and
  `app/tests/words.rs` lists the shared texts that do mean the same.
- **Tor and leaks.** The way to Proton is chosen once per start (`client::Way`); turning Tor on or off relaunches the
  app. Under Tor nothing is looked up locally (`dns::NoLookups`) and there is no fallback. A new way out of the program
  must be listed in `app/tests/network_paths.rs`, and a new page must forbid DNS prefetching. `live_tor_` tests reach
  the Tor network.
- **The target folder.** Debug builds keep line tables only, and none for dependencies: with arti, full debug info
  grew `target` past 80 GB. Cargo also keeps every build of a crate, one for each change of dependencies or features:
  `scripts/prune-target.py`, at the end of `check.sh`, keeps the newest four of our own.
- **Proton registry.** The `proton-crypto-rs` crates need the registry in `.cargo/config.toml`.
- **Sync.** The interval is 30 seconds, as in Proton's clients. Polling more often is not allowed.

## Rules for working with Proton

- The client introduces itself honestly: `x-pm-appversion: Other` and a `User-Agent` with its name and author. It does
  not pose as an official client: `web-mail@...` led to code 2028.
- CAPTCHA and human verification are passed only by a person, on Proton's own page. Protection is never bypassed.
- Check the protocol against Proton's sources (`gh search code`, `gh api`), never from assumption. The record is
  `docs/PROTOCOL.md`: update it with every new API call.
- A failed sign-in is never retried by the app or by a script: failures can lock the account.

## Code and design

- Rust: edition 2024, the version is pinned in `rust-toolchain.toml`. The release profile matters: performance and
  memory come first, and every change to it needs a measurement.
- Interface text in en, ru and de: new keys go into `say.slint` and both language files.
- Strict flat design: no gradients or decorative shadows on buttons. The gradient logo stays. It is a plain envelope,
  blue and never Proton's purple, and it comes from `scripts/logo.sh`.
- In Rust logic, third-party file names, links and HTML are hostile input.
- No `unwrap` and no slicing of strings in Rust: clippy denies both, because a panic on mail is a crash someone else
  can trigger. Use `split_at_checked` or bytes.
