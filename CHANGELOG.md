# Changelog

Every release and what it changed for the people who use it. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org).
A fixed vulnerability is always listed, with its advisory.

## [Unreleased]

The first release, `0.1.0`, is being prepared.

### Added

- Signing in with SRP, a second factor and Proton's human verification, passed by a person on Proton's own page.
- Reading, searching and sorting mail by folders, labels and conversations; folders a page at a time or as one list.
- Writing, replying, forwarding, drafts, scheduled and undoable sending, password-protected and expiring mail.
- Attachments with a preview chosen by the file's bytes, printing with check codes and seals, and a picture of a
  whole mail to copy or share.
- Filters with a Sieve editor, blocked senders, unsubscribing, read receipts, snoozing, contacts and sessions.
- Tor, through Proton's onion site or an exit relay, with the circuit shown.
- An app password with Touch ID on macOS, mail kept on disk sealed, and a journal that holds nothing personal.
- English, Russian and German.

### Security

- Mail is cleaned in Rust before a webview with scripts off and no network shows it; remote images come only through
  Proton's proxy, when asked for.
- Only Proton's own TLS keys are accepted, names are looked up only through Quad9 over DoH, and redirects are refused.

[Unreleased]: https://github.com/koval01/pm-light/commits/main
