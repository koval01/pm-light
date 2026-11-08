# Contributing

Thank you for wanting to help. Proton Mail Light is an unofficial client: it is not affiliated with Proton AG, and
nothing here is Proton's. [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) holds the project's full rules; this page is the
short version.

## Before you start

- Open an issue first for anything larger than a fix, so that the work is not done twice or turned down late.
- Security problems never go into a public issue: see [the security policy](.github/SECURITY.md).

## The rules that are never bent

- **Only Rust.** Everything the app runs is Rust. Scripts around it (checks, builds, bundling) may be shell or Python.
- **English only** in code, comments, logs, tests, documentation and commit messages. Other languages live only in
  the translation files, `app/lang`. `app/tests/english_only.rs` enforces it.
- **No `unwrap`, `expect`, `panic!`, indexing or string slicing that can fail** outside tests: a panic on mail is a
  crash someone else can trigger. Casts go through `app/src/numbers.rs` or `try_from`.
- **Honest towards Proton.** The client introduces itself as what it is. It never poses as an official client, never
  passes or bypasses a CAPTCHA or human verification, and never retries a failed sign-in by itself.
- **Nothing of a person in the journal**: no addresses, subjects, file names, tokens or server messages.
  `app/tests/journal_privacy.rs` checks it.
- **Mail is hostile input**: third-party HTML, file names and links are treated as attacks.

## Checking a change

```bash
scripts/check.sh       # dependencies, lints with warnings as errors, every test
scripts/platforms.sh   # Linux tests, Linux and Windows lints, in Docker
scripts/attacks.sh     # hostile input, in Docker only
```

- Every change comes with tests: the normal path, the edges, hostile input and failure. Prove a new test by breaking
  the code and watching it fail.
- A change to the window is looked at at 900 by 580 and at full screen. If it is meant to be seen, write the screen
  pictures anew with `UPDATE_SCREENS=1 cargo test -p proton-mail-light --test screens`, look at them, and commit them.
- A new interface text goes into `app/ui/say.slint` and both `app/lang/{ru,de}` files.
- A new dependency means running `scripts/components.py`.
- A new call to Proton's API is checked against Proton's own sources and recorded in `docs/PROTOCOL.md`; a change to
  security is recorded in `docs/SECURITY.md`.

## Licence

The project is under the AGPL-3.0 with additional terms (`NOTICE`). By contributing, you agree that your work is
published under the same terms.
