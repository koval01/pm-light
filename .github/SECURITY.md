# Security policy

Proton Mail Light handles decrypted mail and a Proton session, so security problems are taken seriously.
[`docs/SECURITY.md`](../docs/SECURITY.md) describes what the client defends against, how, and its known limits.

## Supported versions

Only the newest release is supported. A fix is released as a new version, not as a patch to an older one.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through
[GitHub's private vulnerability reporting](https://github.com/koval01/pm-light/security/advisories/new), with:

- what an attacker can do, and what they need for it (a mail they send, the network, the computer);
- the steps, a crafted input, or a proof of concept;
- the version and the system.

Never send real mail, real credentials, tokens or anyone's private data, and never test against an account that is
not yours. Crafted input is enough.

You get an answer within a week. A confirmed problem is fixed, released and then disclosed together with you, and
you are credited unless you ask not to be.

## Out of scope

- Proton's own servers, apps and website: report those to [Proton](https://proton.me/security/bug-bounty).
- Malware already running as the same user as the app: see "Known limits" in `docs/SECURITY.md`.
