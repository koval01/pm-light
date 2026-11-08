# Security

What this client defends against, how, and what it does not. It records the findings of an audit and what changed since.

## Who the attackers are

| Attacker                              | What they control                                                     |
| ------------------------------------- | --------------------------------------------------------------------- |
| A stranger sending mail               | Everything in a message: HTML, CSS, headers, MIME, names, attachments |
| A person at the computer, app locked  | The keyboard and the lock screen                                      |
| A network, or a certificate authority | The connection, and possibly a valid certificate for proton.me        |
| Malware running as the user           | Files, and the process if the system allows it. Not defended, see end |

## Layers

- **Mail** is cleaned in Rust by an allowlist, then shown in a webview of its own with scripts off and a policy of
  `default-src 'none'` that lets in only pictures carried in the mail. Each layer is meant to hold alone.
- **The window** is drawn by Slint in this program. No web page holds the app's state, tokens or keys, and there is no
  bridge from a page to them.
- **The composer** edits in a webview of its own, whose one script carries a fresh nonce. It reports only the letter's
  text, focus and pastes, as plain messages that Rust reads, and what enters it is cleaned further by `for_editor`: no
  `<style>`, no `id`, `name` or foreign `class`, and the box contains whatever inline styles remain.
- **The connection** accepts only Proton's own keys, speaks TLS 1.3 alone, ignores proxies, and follows no redirects.
  A refused key is shown as a possible interception, not as a lost connection, and no fallback relaxes the check.
  Sign-in has a second layer: Proton's SRP modulus is verified, and the session is taken only after the server proves
  it knows the password's verifier, which a false server cannot do.
- **Passwords in the native window** pass through the toolkit's strings, which cannot be overwritten. They stay in the
  field while a step runs, are cleared when it ends, and elsewhere live in `Zeroizing` buffers.
- **A `mailto:` link** can come from any page and only fills in a letter for a person to read. Only recipients, subject
  and body are taken: `attach=` and the like are ignored, the body is escaped, a link over 64 KiB is refused, and a link
  that arrives while the app is locked waits until it is opened.
- **Attachment previews** go by the file's bytes, never by what its sender says (`file_kind.rs`). A program is never shown,
  whatever its name, and a file whose bytes differ from its name says so. What is shown is served by a protocol of its own
  that has this one file and nothing else, with the type that was found, `nosniff` and a policy that loads only from itself.
  Scripts are off, no navigation leaves it, nothing is kept on disk, and the engine goes when the sheet closes. Pictures,
  video, sound and PDF are decoded inside WebKit's sandboxed processes, not in this program. Thumbnails are the exception:
  they are decoded here by the `image` crate, memory-safe Rust on a worker, after the size check. This is no virus scanner:
  a file that passes is only a file that was recognised, and saving it and opening it elsewhere is not protected by this.
- **Image metadata** leaves a picture attached from disk when the account asks, by `image_metadata.rs`. It goes by the
  file's first bytes and cuts the file by allowlist: JPEG segments, PNG and WebP chunks, and anything after the end. A file
  it cannot parse goes as it is, so a broken picture may keep its metadata. HEIC is not handled, as in Proton's client.
- **Search patterns** run in the `regex` crate, which is linear in time, and are limited to 256 characters and 256 KiB
  compiled. A pattern is checked only against the rows of a page, and one step reads 1000 messages at most.
- **Recipient domains** are looked up the same way while a letter is written, to mark a domain that takes no mail. Quad9
  sees that domain, and only that. A failed lookup marks nothing.
- **Names** are resolved only by Quad9 over DNS-over-HTTPS, whose certificate must chain to DigiCert's roots. Its
  addresses are built in and the system's resolver is never asked. A false answer gains nothing against Proton's API:
  the key check above still has to pass.
- **A mail in the webview** has scripts off, the policy above, a navigation filter, and a data store that is never
  written to disk. The few lines that fit its height and scroll it are run by the program, not by the page. Canvas, WebGPU, the page cache and the GPU process are off too, which also leaves less
  of WebKit to attack. The switches for those are private, so each is checked for and a system without them only
  uses more memory: the policy above does not depend on them.
- **The account password in settings.** The way back into an account, the second factor and Sentinel ask for it. The
  person types it into a card on the page, it goes through SRP with `ServerProof` checked, and it is not kept: it is
  zeroed after the call and the field goes when the page is drawn again. The raised scope is dropped with
  `PUT /core/v4/users/lock` right after. A refusal is shown and never retried by itself. Changing the password or the
  keys stays on Proton's site, where a mistake cannot cost the mail. A secret for an authenticator app and recovery
  codes live in memory only until the page is left, and are not written to the journal.
- **Settings** go out through a closed table in `mapping/settings.rs`: a key and a value that are not in it never reach
  the server, whatever the window sends.
- **Pictures on disk.** A remote picture that was shown, and Proton's answer about its tracker, are sealed in the mail
  store with the mail. They follow the same switch and the same erasing.
- **Verification** runs Proton's page in a native webview with no access to the app's commands, a private data store,
  and no way to stay on a page outside proton.me.
- **The session** is in the Keychain, sealed with Argon2id and XChaCha20-Poly1305 when an app password is set. Wrong
  passwords cost a pause, then a freeze that survives a restart. It is deleted when Proton refuses it twice, 10 s
  apart, and not because Proton is away or struggling: then the app waits and keeps trying.

- **The journal** holds what the app did, never who or what about: no addresses, subjects, file names, tokens, ids,
  server messages or panic messages. Each record is sealed with XChaCha20-Poly1305 under a random key in the Keychain,
  files are readable by the owner alone, and only the settings page of the window reads them. Nothing outlives a
  day: records older than that go at start, even without the key, and quitting erases everything.

- **Mail at rest.** Read mail and folder lists stay on disk for the next start, each file sealed on its own with
  XChaCha20-Poly1305. File names are keyed hashes and reveal nothing. The key sits inside the saved session, so an app
  password protects it as it protects the session, and a locked app holds no key. Signing out or turning the store off
  erases it. It trims itself to 256 MiB and 30 days. Unlike the journal, it survives quitting, because that is its
  purpose. In memory, 16 messages or 16 MiB stay ready, and settings answers are kept until a change or a sign-out.
  Spam, trash and drafts are never read ahead, and reading ahead marks nothing as read and loads nothing remote.
- **Contact data on screen.** Recovery contacts, payment details and addresses in the sign-in log stay under moving dust
  until pointed at, against someone looking over a shoulder. It is not access control: they are in the window.
- **Tor**, when chosen, carries everything the app sends: Proton's API, Quad9 and Proton's verification page. It runs
  inside the program as the Tor Project's own arti, with vanguards for onion services, and keeps its guards between
  starts. The default route is Proton's onion site, `mail-api.protonmailrmez3lotccipshtkleegetolb73fuirgj7r4o4vfu7ozyd.onion`,
  so a connection never leaves Tor; its certificate is pinned with its issuer's key. The other route leaves at an exit
  for `mail-api.proton.me`, under Proton's own pins. What keeps anything from going around it:
  - The way to Proton is chosen once, before the first request, and never changed: turning Tor on or off starts the
    app again, so no request of the old way outlives the change. Until it is chosen, nothing goes out.
  - Under Tor, no name is looked up on this machine: every connection goes into Tor by name, and the resolver the
    HTTP clients carry refuses whatever reaches it. Quad9 goes into Tor by its built-in address.
  - If Tor cannot start, the app stays offline and says so. There is no fallback to a direct connection.
  - The door to Tor is a SOCKS5 server on the loopback interface, with a random password for each start. Proton,
    Quad9 and the verification page have circuits of their own, as Tor's `IsolateSOCKSAuth` keeps them, and each may
    reach only its own hosts: Proton's names on port 443, Quad9's addresses. The page's door has no password, since a
    webview cannot give one, and is open only while the page is.
  - The client for Proton hands names to the door as names (`socks5h`), never as addresses it looked up itself. A
    test reads what the app's own HTTP client sends to a SOCKS server and fails if a name is resolved first.
  - New circuits cut every stream open through the door, and the HTTP clients let their kept-alive connections go:
    nothing goes on over a circuit the person asked to leave. A live test fails without the cut.
  - Circuits follow Tor's own rule: new connections take new circuits after ten minutes, and nothing is rotated
    faster, which would only add relays to be chosen from. A circuit to Proton that stays slow is replaced: when the
    middle of the last five answers is over five seconds, Quad9 is asked over its own circuit, and if it answers
    within three, the circuit is to blame. Nothing is asked outside Tor to tell the two apart.
  - Hostile input at the door (thousands of random and cut-short openings, names that are not text, a client that
    sends a byte and waits) is turned away in bounded time, in the attack run.
  - The verification page has WebRTC turned off, which could send UDP past any proxy.
  - Every page the app writes forbids DNS prefetching, by which an engine looks up the names of a page's links. This
    holds without Tor too, where it would have gone past Quad9.
  - Links open in the browser, outside Tor, only after the person agrees, each time.
  - Pictures from Gravatar are off unless the person turns them on, after a warning: Gravatar would learn, in coded
    form, the addresses of the people who write to them. Turned on, they come through Proton's image proxy, so
    Gravatar sees Proton and not this computer.
  - Remote pictures and logos come through Proton's image proxy over the same way: the sender sees Proton's server,
    and Proton sees Tor.
  - `app/tests/network_paths.rs` names every way out of the program and the one file it may live in. A new one fails
    the build until it is routed and listed.
- **A picture of a mail.** Copying a mail as a picture, or sharing it, takes it out of Proton's encryption: the
  person is told so once, before the first. It is drawn as printing draws it, in a webview out of sight with scripts
  off and a policy that lets in nothing but the document's own data, and with remote pictures only when the reader
  shows them. A shared picture is written as `mail.png`, a name that tells nothing of the mail, into the cache folder
  that quitting empties. Nothing of it goes to the journal but its size. `--check-picture=<file>`, for CI, draws a
  made-up mail the same way and writes it to that file; it puts it on the system's clipboard on Windows and Linux,
  and on a pasteboard of its own on macOS.
- **The repository.** Actions are pinned to commits, checkouts keep no credentials, and every workflow starts with
  read-only permissions. gitleaks reads every commit for secrets (one published test key is allowed by path, in
  `.gitleaks.toml`), zizmor reads the workflows, cargo-deny the dependencies, daily. Dependabot proposes updates
  weekly, each a week after its release. CodeQL reads the Rust and the workflows with both of its extended suites,
  the OpenSSF Scorecard rates how the repository is kept, pull requests get a dependency review, and secret scanning
  refuses a push that carries a secret. `main` takes changes only through pull requests that pass these checks.
- **The process.** Core dumps are off. On Windows a crash report leaves the heap out, and libraries are not loaded
  from a network share or a low-integrity folder, with the system's own preferred. On macOS a release build refuses debuggers (`PT_DENY_ATTACH`), and on Linux the
  process is not dumpable. A second start hands its link to the first copy over a local socket and quits: a message
  over 256 KiB is dropped, and only `mailto:` links are taken from it.

## Interception

| Attack                                                 | What stops it                                      | Checked by                   |
| ------------------------------------------------------ | -------------------------------------------------- | ---------------------------- |
| A hacked or coerced authority issues a cert for Proton | The key must be one of Proton's three              | Rogue CA test, and live      |
| A local CA added by a company or malware               | The system's trust store is never read             | Roots are built into the app |
| Forcing TLS 1.2                                        | TLS 1.3 only                                       | A TLS 1.2 server test        |
| A proxy variable on the machine                        | Proxies are ignored                                | A proxy test                 |
| A false DNS answer for Proton                          | Quad9 over TLS, and the key check above regardless | False server test            |
| Someone posing as Quad9                                | Only DigiCert's roots are trusted for it           | False server test, and live  |
| A redirect to take the session's headers elsewhere     | Redirects are refused                              | A redirect test              |
| A false server at sign-in                              | SRP: the server must prove itself                  | Compared before any session  |

The journal records a refused certificate, and the app says so on screen instead of "no connection".

## Checks

```bash
scripts/attacks.sh   # hostile input in Docker, with no network
```

`html/parsed_tests.rs` crosses known mutation-XSS shapes from `html/hostile_fragments.json` into 6000 messages. The
sanitizer cleans them in the container, then the result is parsed by html5ever, the HTML standard's algorithm in Rust,
written out and parsed again, because mutation attacks live in that second reading. Its CSS is read by a CSS tokenizer,
as an engine reads it. The test fails on any tag, attribute, handler, address or CSS the allowlist does not permit,
and two ordinary tests prove it catches each kind and leaves clean mail alone.

html5ever is not an engine: an engine's own parsing quirks are not covered.

`scripts/check.sh` runs before every build, locally and in CI: `cargo deny` for known vulnerabilities, yanked crates,
licenses and sources, then clippy with its pedantic and nursery groups and warnings as errors, then every test.
Clippy denies `unwrap`, `expect`,
`panic!`, `unreachable!`, indexing and string slicing that can fail, and `process::exit`, outside tests. Patterns
written into the code go through `pattern.rs`, which matches nothing rather than panicking, and a test compiles each.
GitHub's CodeQL scans the Rust code and the workflows on every push and weekly. Each finding below has a test that
fails on the old code.

## Findings of the audit

All are fixed unless listed under [Known limits](#known-limits).

| Severity | Finding                                                                                                | Fix                                                     |
| -------- | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------- |
| High     | Removing a CSS comment rebuilt `</style>` and let markup out of the sanitizer                          | Comments become a space; `<` is escaped in style blocks |
| High     | Replying or forwarding put a mail's `<style>`, ids and classes into the app's own document             | `for_editor`, and containment on the editor             |
| High     | A header name with a Kelvin sign, or a type with a multi-byte character at byte 6, panicked on slicing | Checked cuts; clippy denies string slicing              |
| Medium   | `ht\ntps://`, `https:host` and similar images escaped the hold on remote images; CSP stopped the fetch | Addresses are read the way a browser reads them         |
| Medium   | A name like `a.exe.` or `a.e\u{202e}xe` passed the program check and was saved as `a.exe`              | The check judges the name that reaches the disk         |
| Medium   | Redirects were followed with the session's headers                                                     | Redirects are refused                                   |
| Medium   | Any trusted certificate for proton.me was accepted                                                     | Proton's keys are pinned                                |
| Medium   | Secrets stayed in memory: passwords, and the key secret in saved sessions                              | `Zeroizing` on each path                                |
| Medium   | A token refresh landing during a lock could save the session unsealed, or bring it back into memory    | Secrets go first, and a refresh after a lock is dropped |
| Medium   | The app password could be guessed without limit                                                        | A pause and persistent freezes                          |
| Medium   | A proxy in the environment was obeyed, which would have taken names away from Quad9                    | Proxies are ignored                                     |
| Medium   | Any 4xx to a token refresh, 429 and 409 among them, deleted the session; any error at startup did too  | Only 400/401/422 count, twice, 10 s apart               |
| Medium   | A 401 from the refresh itself waited for its own lock and hung every request for good                  | The refresh path never refreshes                        |
| Low      | Sleep did not count as being away, so a closed laptop with a focused window stayed unlocked            | Time asleep counts                                      |
| Low      | A sender's display name could add lines to a contact's vCard                                           | Line breaks are escaped, control characters dropped     |
| Low      | `CON` and other device names, invisible characters and padding survived in saved file names            | Handled, and a long name keeps its extension            |
| Low      | A link dialog showed `https://bank.example@evil.example/` with no hint of the real host                | The host comes first                                    |
| Low      | Network errors could carry a mail's image address into logs                                            | Errors drop the address                                 |

The runnable list also grew to cover macOS links and profiles, macros, scripts, and HTML files, which open in a browser
with no sanitizer in front of them.

## Known limits

- **Page scripts on Linux.** Elsewhere a page's JavaScript is off entirely, and the app's own calls run in a world of
  their own. WebKitGTK's switch for that stops the app's calls too, so on Linux only the scripts of the markup are
  turned off, right after the webview is made. Until then, and always, the page's policy (`default-src 'none'`)
  forbids every script, and the mail has been cleaned of them.
- **Malware running as the user, an administrator, the kernel.** They can read process memory or the webview's. Core
  dumps are off and debuggers are refused where the system allows it, and that is all a client can do.
- **Touch ID is a software gate.** The key that opens the session sits in the Keychain next to it, so reaching the
  Keychain outside the app defeats the seal once Touch ID is on. A biometric access control needs a signed build with
  a Developer ID, which this project does not have yet.
- **The toolkit's strings cannot be wiped.** A password typed into a Slint field lives in its strings until they are
  freed. The mail's webview frees a mail when the engine chooses.
- **Password-protected mail** keeps its password as a plain string while sending.
- **Key trust is the server's word.** Recipient keys come from `/core/v4/keys`, with no key transparency check.
- **A lost refresh reply.** If a lock or a dropped connection discards rotated tokens, Proton may refuse the old ones.
  The client cannot tell that from a revoked session, so the person signs in again, with an explanation.
- **The journal's reach.** Whoever can read the Keychain entry can read the journal. A file erased from an SSD can
  linger in free blocks, though only as ciphertext. A crash leaves the journal on disk until the next quit or a day, as
  does an app killed by force, because that record is what explains the failure.
- **Quad9 can go stale too.** If it moves to another certificate authority, or is blocked on a network, the client
  stays offline until an update or another network. That is the price of having no fallback.
- **The verification page** runs in the system's webview, which resolves names the system's way. Only that page does.
- **A seal cannot see paper.** It proves what the signer's address sealed, and shows it to compare. Text changed on the
  sheet leaves the codes intact, and only that comparison finds it. A sheet can be copied whole, seal included. The key
  comes from Proton, so a seal says "Proton lists this key for this address", and nothing about who owns the address.
  Scans and PDFs are hostile input, and two QR decoders read them on a worker. PDFs yield only their embedded pictures,
  with sizes bounded before anything is unpacked. It is not an ICAO or BSI visible digital seal, which need a state's certificate and code. Its format is standard
  OpenPGP, checked with GnuPG from a printed page.
- **The simple check code** only ties a sheet to a message in this account, and proves nothing to anyone outside it.
- **Pins can go stale.** If Proton retires all three keys, the client reports that it cannot reach Proton until an
  update. That is the price of the check, and Proton's Bridge pays it too.
- **Response sizes are not capped.** Only a server holding a pinned key could abuse this.
- **Printing** puts each cleaned mail inside two nested shadow roots of a webview out of sight, so a mail's styles reach
  only its own part of the printout.
- **Windows and Linux** compile and pass the logic tests, but no one has run the app on them. On Windows there is no
  equivalent yet of the macOS debugger block, and crash reports may hold process memory. Without a Secret Service on
  Linux there is nowhere to keep the session or the journal's key, so sign-in does not survive a restart.
- **Tor's limits.** Proton still knows which account asks: Tor hides where a person is, not who signs in. Links opened
  in the browser leave Tor, as the app warns. The verification page runs Proton's scripts in the system's webview,
  whose engine Tor does not harden as Tor Browser does. Choosing Tor on a network that blocks it leaves the app
  offline: bridges are not offered yet. arti's GeoIP feature, which names the countries of a circuit, is marked
  experimental by the Tor Project.
- **Unsigned builds.** There is no Developer ID, notarization or hardened runtime, so nothing protects the installed
  app from being replaced by something running as the user.

### Advisories in dependencies

`cargo audit` reports three, all through the `pgp` crate inside Proton's own crates, which are at the newest revision
(`c468e28`). None has a fix available here.

| Crate                 | Advisory          | Effect here                                                                                                                                                     |
| --------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `rsa` 0.9.10          | RUSTSEC-2023-0071 | Marvin timing attack. It needs many measured decryptions. This client decrypts locally and reveals no timing to a sender, since opening a mail contacts nobody. |
| `crypto-bigint` 0.5.5 | RUSTSEC-2026-0387 | Non-constant-time operations, reached through `cx448`. Same reasoning; only keys of that type are affected.                                                     |
| `ml-dsa` 0.0.4        | RUSTSEC-2025-0144 | Timing in post-quantum signing, which an attacker would have to measure locally.                                                                                |

GitHub's Dependabot reports more, from GitHub's own advisory database:

| Crate          | Advisory            | Effect here                                                                                                                         |
| -------------- | ------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `ml-dsa` 0.0.4 | GHSA-5x2r-hc65-25f9 | A post-quantum signature with repeated hint indices is accepted. Reached through Proton's `pgp`; fixed only when Proton moves it.   |
| `ml-dsa` 0.0.4 | GHSA-h37v-hp6w-2pp8 | An off-by-two in `UseHint` when `r0` is zero. Same path, same wait.                                                                 |
| `glib` 0.18.5  | GHSA-wrw7-89jp-8q8g | Unsound `VariantStrIter`, Linux only, through the `gtk` 0.18 wry is pinned to. The app never iterates a `Variant`'s strings.         |
| `lru` 0.7.8    | RUSTSEC-2026-0253   | A use-after-free when a key's or value's `Hash` or `Drop` panics, inside `reed-solomon-erasure`. Its keys are `Vec<usize>`, its values `Arc<Matrix>`: neither panics. |

`osv-scanner.toml` lists each of these with its reason, for osv-scanner and the OpenSSF Scorecard, and asks for each
to be looked at again by a date. The rest are `unmaintained` notices for Linux-only and build-time crates. `cargo deny` also found two
advisories in `hickory-proto` 0.25 (RUSTSEC-2026-0118 and 0119): the first needs DNSSEC validation, which is not
built in, and the second only slows the encoding of the app's own queries. It was moved to 0.26.3 regardless.
