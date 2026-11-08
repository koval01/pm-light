# Checking against the Proton protocol

Every call and rule was checked against source code, not memory. Sources:

- **Go**: [go-proton-api](https://github.com/ProtonMail/go-proton-api), the client library of Bridge.
- **SRP**: [go-srp](https://github.com/ProtonMail/go-srp).
- **Web**: [WebClients](https://github.com/ProtonMail/WebClients), the official web client.

Status words: **matches** — as in the source; **fixed** — the check found a difference and the code now follows the
source; **assumption** — the sources disagree or say nothing, and the decision is described.

Confirmed on a real account: sign-in with `x-pm-appversion: Other`, session restore, folders, decrypting
mail and attachments, sending to an external address, password-protected mail. Everything else is checked against
source only; see [What only the live server can confirm](#what-only-the-live-server-can-confirm).

## Sign-in and session

| What                                                                                                                          | Source                          | Status                                                                          |
| ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------- | ------------------------------------------------------------------------------- |
| `POST /auth/v4/info`, body `Username`                                                                                         | Go; `Intent: "Proton"` from Web | fixed: `Intent` added                                                           |
| `POST /auth/v4`: `Username`, `ClientEphemeral`, `ClientProof`, `SRPSession`                                                   | Go                              | matches                                                                         |
| `ServerProof` check in constant time                                                                                          | SRP                             | fixed                                                                           |
| SRP: little-endian, `expandHash`, multiplier, bounds, safe prime, modulus signature                                           | SRP                             | matches, checked on a real modulus                                              |
| bcrypt: `$2y$10$`, 22 salt characters, salt returned as is                                                                    | SRP                             | fixed                                                                           |
| Auth versions 3 and 4                                                                                                         | SRP                             | matches; 0–2 are refused with an explanation                                    |
| `2FA.Enabled`: bit 1 TOTP, bit 2 FIDO2                                                                                        | Go                              | fixed: FIDO2-only closes the session with a message                             |
| `POST /auth/v4/2fa`, `TwoFactorCode`                                                                                          | Go                              | matches                                                                         |
| `POST /auth/v4/refresh`: all fields and `AccessToken`                                                                         | Go                              | fixed                                                                           |
| `DELETE /auth/v4`                                                                                                             | Go                              | matches                                                                         |
| Headers `x-pm-appversion`, `x-pm-uid`, `Authorization: Bearer`                                                                | Go                              | matches                                                                         |
| A session's name in Proton's list is `LocalizedClientName`, set by the server from the client: "Unknown" for `Other`          | Web                             | nothing a client can send                                                       |
| Mail is found by polling every 30 s, as `INTERVAL_EVENT_TIMER` in Proton's web client. No held-open request is in its sources | Web                             | matches                                                                         |
| `x-pm-appversion: Other`                                                                                                      | protonlite                      | assumption: absent from Go and Web; accepted by the live server                 |
| 9001: `HumanVerificationToken`, `HumanVerificationMethods`, `WebUrl`                                                          | Go, protonlite                  | matches                                                                         |
| Human verification headers                                                                                                    | Go, Web                         | assumption: Go sends all methods comma-separated, Web one; we send one          |
| 401 → refresh; only 400/401/422 on refresh rule the session invalid; 409, 429, 5xx mean later                                 | Web (`refreshHandlers.ts`)      | matches, and a refusal is asked once more after 10 s before anything is deleted |
| 429 and 503 with `Retry-After`, 10 s by default                                                                               | Go                              | fixed                                                                           |
| Codes 1000, 1001 for batches only, parsing `Responses`                                                                        | Go                              | fixed                                                                           |

## Human verification

| What                                                                                 | Source | Status                                                             |
| ------------------------------------------------------------------------------------ | ------ | ------------------------------------------------------------------ |
| `WebUrl` opened as a full page, with `locale` and `theme` added                      | Web    | matches; page and captcha frame load, no real challenge solved yet |
| The page's result: `HUMAN_VERIFICATION_SUCCESS` with `token` and `type`, and `CLOSE` | Web    | matches                                                            |
| A captcha on the full page is redeemed with `POST` of `submitExternalCaptcha`        | Web    | matches: the retry sends the challenge's own token                 |
| Other methods hand back a token, which the retry sends with its `type`               | Web    | matches                                                            |
| Browser link as an alternative, sign-in resumes when the window regains focus        | Web    | matches                                                            |

The page cannot sit in an iframe: its CSP has `frame-ancestors https://mail.proton.me https://calendar.proton.me
https://drive.proton.me`. That limit is respected. The page runs at the top of a native child webview inside the
app's window (`app/src/verify.rs`), the way Proton's own apps run it, unchanged and in a private data store.
It has no channel to the program: it can only navigate to `proton-mail-light-verify://`, which reports the outcome. The client never solves
the check itself.

Proton's page reports to `window.parent`, which at the top of a webview is the page itself. A small script listens for
that and turns the result into a navigation that Rust stops and reads. On WebView2 (Windows) the page takes
`chrome.webview` for a host and posts nothing, so this needs its own answer before a Windows release.

## Reachability

`GET /tests/ping` answers `{"Code":1000}` without a session, checked on the live server. It is called only while the
client is offline, to notice the network coming back. While online, the regular sync is the signal, so Proton sees no
extra traffic.

## Keys and encryption

| What                                                      | Source     | Status                                                              |
| --------------------------------------------------------- | ---------- | ------------------------------------------------------------------- |
| OpenPGP library                                           | Web        | fixed: Proton's own crates (`proton-rpgp`, `proton-crypto-account`) |
| One key password, from the salt of the primary key        | Go, Web    | fixed                                                               |
| No salt: the password itself is the key password          | Web        | fixed                                                               |
| The last 31 bytes of bcrypt                               | Go, SRP    | matches                                                             |
| `Active` keys only                                        | Go         | matches                                                             |
| `Token` and `Signature`: decryption and signature check   | Go         | fixed                                                               |
| A key without a token opens with the key password         | Go         | matches                                                             |
| Decryption as bytes, UTF-8, `\r\n` → `\n`, nbsp → space   | Web        | fixed                                                               |
| `allowForwardedMessages` for flag 2^35                    | Web        | fixed                                                               |
| Key order: the mail's address, other addresses, user keys | protonlite | assumption                                                          |
| Signature: none, nothing to check with, valid, invalid    | Web        | fixed                                                               |
| Sender keys from `/core/v4/keys`, except compromised ones | Go, Web    | fixed: pinned contact keys are not considered                       |
| Attachments: key and data packets, detached signature     | Go, Web    | matches                                                             |

## Mail

| What                                                                                                       | Source     | Status                                            |
| ---------------------------------------------------------------------------------------------------------- | ---------- | ------------------------------------------------- |
| `GET /mail/v4/messages`: `LabelID`, `Page`, `PageSize`, `Sort`, `Desc`, `Keyword`, `Unread`, `Attachments` | Web        | matches                                           |
| The same, for search: `From`, `Recipients`, `Subject`, `Begin`, `End`, `Starred`                           | Web        | matches `queryMessageMetadata`                    |
| `ConversationID` filter                                                                                    | not in Web | fixed: removed                                    |
| `GET /mail/v4/conversations/{id}`                                                                          | Web        | fixed                                             |
| `GET /mail/v4/messages/{id}`                                                                               | Go, Web    | matches                                           |
| `GET /mail/v4/messages/count`                                                                              | Go, Web    | matches                                           |
| System labels 0, 3, 4, 5, 6, 7, 8, 10, 12                                                                  | Web        | matches                                           |
| `PUT read`, `unread`, `label`, `unlabel` with `IDs`                                                        | Go, Web    | matches                                           |
| `PUT delete` with `CurrentLabelID`                                                                         | Web        | fixed                                             |
| `DELETE /mail/v4/messages/empty?LabelID=`                                                                  | Web        | matches                                           |
| Nothing moves from Sent or Drafts to Inbox or Spam                                                         | Web        | fixed                                             |
| `NextMessageOnMove`                                                                                        | Web        | fixed                                             |
| PGP/MIME: body and attachments from MIME                                                                   | Web        | fixed                                             |
| Remote images: `GET /core/v4/images?Url=&DryRun=0`                                                         | Web        | fixed                                             |
| Gravatar, when turned on: the same, for `www.gravatar.com/avatar/<sha256>?s=128&d=404`                     | This app   | opt-in                                            |
| `Total` of a list reply: the folder with its filters, for the number of pages (`elementQuery.ts`)          | Web        | matches                                           |
| Inline images by `content-id`                                                                              | Web        | matches                                           |
| HTML cleaning                                                                                              | Web        | same approach as DOMPurify, our own rules in Rust |

## Sending

| What                                                                 | Source  | Status                                       |
| -------------------------------------------------------------------- | ------- | -------------------------------------------- |
| `POST /mail/v4/messages`, `PUT /mail/v4/messages/{id}`               | Go, Web | matches                                      |
| `ParentID`, `Action`: 0 reply, 1 reply all, 2 forward                | Go      | matches                                      |
| One package per MIME type, `Type` as an OR of schemes                | Go, Web | fixed                                        |
| Schemes: 1 inside Proton, 4 clear                                    | Go, Web | matches                                      |
| `Signature: 1` for internal mail when all is signed; 0 for clear     | Web     | fixed                                        |
| `BodyKeyPacket`, `AttachmentKeyPackets`, `BodyKey`, `AttachmentKeys` | Go, Web | matches                                      |
| The body is signed and encrypted once                                | Web     | matches                                      |
| Signature date for a scheduled send                                  | Web     | fixed                                        |
| `POST /mail/v4/messages/{id}`: JSON                                  | Go      | matches; Web sends a form                    |
| `DeliveryTime`, `AutoSaveContacts`                                   | Web     | fixed                                        |
| Choosing the recipient key by `Flags`                                | Web     | fixed                                        |
| `GET /core/v4/keys?Email=`                                           | Go      | assumption: Web moved to `/core/v4/keys/all` |
| `POST /mail/v4/attachments`: fields and the part name `blob`         | Go      | fixed                                        |
| `DelaySeconds`, `POST /mail/v4/messages/{id}/cancel_send`            | Web     | matches                                      |
| Batches of 150 IDs                                                   | Go      | matches                                      |
| Reply from the address in `AddressID`, prefixes `Re:` and `Fw:`      | Web     | fixed                                        |

Replies, forwards and signatures are built in Rust (`core/src/compose.rs`). The interface passes the labels it wants
written into the message, so quoted headers follow the reader's language.

## Conversations

`GET /mail/v4/conversations` takes the query of the message list and answers with `Conversations`, each with `Senders`,
`NumMessages`, `NumUnread`, `NumAttachments` and `Labels`, whose `ContextTime`, `ContextNumUnread` and
`ContextNumAttachments` say what it holds in one folder. `GET /mail/v4/conversations/count` replaces the message counts.
`PUT /mail/v4/conversations/read`, `unread` (`LabelID`), `label`, `unlabel` and `delete` (`LabelID`) take `IDs`. A search and
the labels in `ALWAYS_MESSAGE_LABELS` stay messages (`isConversationMode`). `StickyLabels` is applied by the server.

## Signature footer

`PMSignature` is two bits: 1 turns Proton's line on, and 2 says the plan does not let it be turned off. The line is added
by the client under the address's signature (`messageSignature.ts`), as `Sent with Proton Mail secure email.`, and it goes
out as part of the body. `PMSignatureContent` and a referral link are not used here.

## Embedded pictures

`HideEmbeddedImages` set to 1 keeps embedded pictures off the page until asked. Drafts and mail whose `Sender.IsProton` is
set are exempt (`transformEmbedded.ts`). The pictures are decrypted either way: nothing more is requested when they are shown.

## Plain text letters

A draft's `MIMEType` is `text/plain` for a new letter when `DraftMIMEType` says so, and otherwise that of the mail it
starts from (`messageDraft.ts`). The body is the text itself: four empty lines, the signature as text, a blank line, and
what is answered with `> ` before each line (`draftContent/plaintext.ts`). The package carries the same `MIMEType`.

## Settings, labels, contacts

The list follows the account's mail settings: `ShowMoved` reads Drafts and Sent from labels `1` and `2` in place of `8`
and `7`, which then hold what was moved out of them as well, and `AlmostAllMail` reads All Mail from `15` in place of
`5`. `PageSize` goes no higher than 150, whatever the setting says.

| What                                                           | Source  | Status                                 |
| -------------------------------------------------------------- | ------- | -------------------------------------- |
| `GET`, `POST`, `PUT`, `DELETE /core/v4/labels`, `Type` 1 and 3 | Go, Web | matches                                |
| A rename passes `Name` and `Color`                             | Go, Web | fixed                                  |
| The color `#8080FF` is in the palette                          | Web     | matches                                |
| `GET /contacts/v4/contacts/emails`                             | Web     | matches; Go uses `/contacts/v4/emails` |
| `GET /core/v4/users`, `/addresses`, `/keys/salts`              | Go      | matches                                |
| An address can send: `Status = 1`, `Send = 1`, has keys        | Go      | matches                                |
| `PUT /core/v4/addresses/order`                                 | Go, Web | matches                                |
| `PUT /core/v4/addresses/{id}`: `DisplayName`, `Signature`      | Web     | matches                                |
| `PUT /mail/v4/settings/delaysend`: 0, 5, 10, 20                | Web     | matches                                |
| `GET /mail/v4/settings` and the separate settings paths        | Web     | matches                                |
| `NextMessageOnMove`: 0 off, 2 on by the user, 1 on by default  | Web     | matches                                |

## Auto-reply, filters, spam

| What                                                                    | Source | Status                                                                                   |
| ----------------------------------------------------------------------- | ------ | ---------------------------------------------------------------------------------------- |
| `PUT /mail/v4/settings/autoresponder`, `Repeat`: 0 dates, 4 no end date | Web    | matches                                                                                  |
| `GET`, `POST`, `PUT`, `DELETE /mail/v4/filters`, `enable`, `disable`    | Web    | matches                                                                                  |
| `PUT /mail/v4/filters/check` before saving, the `Issues` field          | Web    | matches; the shape of `Issues` with a position is an assumption                          |
| Sieve text version 2 with a spam preamble                               | Web    | assumption: the preamble is rebuilt from Proton's tree, and the server checks the script |
| `PUT /mail/v4/messages/{id}/mark/ham`                                   | Web    | matches                                                                                  |
| `POST /mail/v4/incomingdefaults?Overwrite=1`, `Location: 14`            | Web    | matches                                                                                  |
| `POST /mail/v4/messages/{id}/unsubscribe`, `UnsubscribeMethods`         | Web    | matches                                                                                  |

## Protection, snoozed mail, contacts

| What                                                                                     | Source          | Status                                                           |
| ---------------------------------------------------------------------------------------- | --------------- | ---------------------------------------------------------------- |
| Password mail: scheme 2, `Auth`, `Token`, `EncToken`, `BodyKeyPacket` through a password | Web             | matches                                                          |
| `Auth` from `GET /auth/v4/modulus`, 10-byte salt, verifier                               | Go, Web         | matches                                                          |
| `ExpiresIn` in seconds                                                                   | Web             | matches; the 4-week limit is an assumption, the server checks it |
| `FLAG_RECEIPT_REQUEST` in a draft, `POST /mail/v4/messages/{id}/receipt`                 | Web             | matches                                                          |
| Receipt request from the `Disposition-Notification-To` header                            | Web             | matches                                                          |
| `PUT /mail/v4/conversations/snooze`, `unsnooze`, label 16                                | Web             | matches                                                          |
| `POST /contacts/v4/contacts`, `PUT .../delete`                                           | Web             | matches                                                          |
| Signed vCard, type 2                                                                     | Web, protonlite | matches                                                          |

## Account and security

| What                                                                                                                     | Source | Status               |
| ------------------------------------------------------------------------------------------------------------------------ | ------ | -------------------- |
| `GET /core/v4/users`, `GET /core/v4/settings` (`UserSettings`)                                                           | Web    | matches              |
| Plan: `GET /payments/v4/subscription`, only with `Subscribed`                                                            | Web    | matches              |
| Billing, read only: `GET /payments/v5/methods`, `GET /payments/v5/invoices` with `Page`, `PageSize`, `Owner`, `Document` | Web    | not yet checked live |
| Sessions: `GET`, `DELETE /auth/v4/sessions`, `DELETE .../{UID}`, newest first                                            | Web    | matches              |
| Sign-in log: `GET`, `DELETE /core/v4/logs/auth`, `PUT /core/v4/settings/logauth`                                         | Web    | matches              |
| `PUT /mail/v4/settings/confirmlink`                                                                                      | Web    | matches              |
| `PUT /mail/v4/settings/auto-delete-spam-and-trash-days`, body `{ Days }`                                                 | Web    | matches              |

`POST /auth/v4/2fa` takes a recovery code in the same `TwoFactorCode` field as a six-digit code (Web, `TotpInputs.tsx`).

| What                                                                                                           | Source | Status               |
| -------------------------------------------------------------------------------------------------------------- | ------ | -------------------- |
| Sender lists: `GET`, `POST ?Overwrite=1`, `PUT .../delete` on `/mail/v4/incomingdefaults`                      | Web    | not yet checked live |
| Forwarding: `GET /mail/v4/forwardings/{outgoing,incoming}`, `PUT .../{ID}/pause`, `/resume`, `DELETE .../{ID}` | Web    | not yet checked live |
| Domains: `GET /core/v4/domains`, read only                                                                     | Web    | not yet checked live |

Earlier mail from a sender for printing: `GET /mail/v4/messages` with `LabelID=5`, `From`, `End`, `Total` in the reply
(Web, `queryMessageMetadata`; not yet checked live).

Domain checks read the records Proton's domain pages ask for, through Quad9: `TXT` for `VerifyCode` and SPF
(`include:_spf.protonmail.ch`), `MX` (`mail.protonmail.ch` first, `mailsec.protonmail.ch`), `CNAME` for each
`DKIM.Config` row, and `TXT` at `_dmarc`.

Left out of the client on purpose: changing the password, managing two-factor protection and recovery, creating or
exporting keys, and setting up forwarding, which creates keys.
Changing the log mode may ask for password confirmation: the live server will show.

## Message details and protection

| What                                                                          | Source      | Status                                                   |
| ----------------------------------------------------------------------------- | ----------- | -------------------------------------------------------- |
| Message `Size`, headers from `Header`                                         | Web, Go     | matches                                                  |
| Encryption type from `X-Pm-Content-Encryption`                                | Web         | matches; an unknown value is shown as "stored encrypted" |
| Trackers: `GET /core/v4/images?Url=&DryRun=1`, header `x-pm-tracker-provider` | Web         | matches                                                  |
| SPF, DKIM, DMARC from `Authentication-Results`                                | RFC 8601    | matches the headers of live mail                         |
| Link parameters, removed elements, unnamed pixels                             | client side | does not involve the server                              |

With `DryRun=1` Proton does not return the image, it only names the tracker. It sees the addresses anyway when it
loads images through its proxy.

## Where requests go

Rust (`reqwest`) sends requests directly to `https://mail-api.proton.me`, as Proton's apps and Bridge do. The page has
no network: the app's CSP leaves it only IPC. So browser CORS rules do not apply to the app, the `User-Agent` reaches
the server, and response headers are fully visible, including `x-pm-tracker-provider`.

TLS accepts only the keys in Proton's `TrustedAPIPins` for proton.me (`core/src/tls.rs`), on top of the usual chain
check. The live key matched the "current" pin. Redirects are refused: Proton's API does not use them.

Through Tor, the API is reached at Proton's onion site by the rule the web client follows (`getApiSubdomainUrl` in
`packages/shared/lib/helpers/url.ts`: `mail` becomes `mail-api`):
`https://mail-api.protonmailrmez3lotccipshtkleegetolb73fuirgj7r4o4vfu7ozyd.onion`. The bare onion host answers `/api`
with 401; `mail-api.` answers `/tests/ping` with `{"Code":1000}`, when checked. Its certificate is an EV
certificate for the onion name and `*.` of it from DigiCert EV RSA CA G2.

There is no browser release, and every request comes from Rust. For the record, checked in Chromium, Firefox and WebKit
with requests that carry no account: Proton lets a page read responses directly from `localhost`, private network
addresses and `tauri://localhost`, but not from public domains, `null`, extensions or `http://tauri.localhost`.

## Showing mail

Mail is shown in a child webview of its own, with scripts off and a policy of `default-src 'none'`
(`app/src/mail_view.rs`). There are two independent layers of protection: cleaning in Rust (`ammonia`, then
`lol_html`, with `cssparser` for CSS) and that policy. After cleaning, a message holds no
remote addresses: image `src` moves to `data-held-src`, and CSS `url()` is removed.

Limits: 4 MB of HTML, 60,000 tags, nesting depth 512, CSS depth 32. A message past them is shown as plain text and
reported as oversized.

## Local data

Nothing here touches the protocol, but it decides what Proton's data looks like on the device.

- No mail is written to disk. Preferences are in the webview's local storage.
- The session is in the system Keychain. Keys are never stored: they come from the server after sign-in.
- With an app password, the session is an envelope `{salt, nonce, sealed}`: Argon2id (64 MiB, 3 passes) and
  XChaCha20-Poly1305. Without the password the envelope is useless, and without a session Proton gives nothing.

## Checking SRP

Sign-in and keys are handled by Proton's official crates, pinned to one revision of `proton-crypto-rs`. The tests in
`core/src/crypto.rs` run on the key from Proton's example: unlocking with salt and password, encryption,
decryption, send packets, password mail, refusing a forged modulus. The modulus signing key is built into Proton's
crates.

## What only the live server can confirm

1. Signing in again with a human verification token: the server accepted it and answered 8002, and a full sign-in has
   not gone through.
2. The embedded verification page with a real challenge.
3. Sending inside Proton, including `Signature: 1`.
4. Token refresh without a cookie.
5. Old mail and old keys of a real account.

## Left out on purpose

Partial coverage is not a difference. Missing features are listed in the [README](../README.md).

## What Proton shows only on its site

These tabs are shown in the app, from the same calls as `packages/shared/lib/api` of Proton's web client.

- **Invitations.** `GET /core/v4/referrals/status`: `RewardMonths`, `RewardMonthsLimit`, `EmailsAvailable`.
  `GET /core/v4/referrals/info`: `Currency`, `ReferrerRewardAmount`, `RefereeRewardAmount`, `MaxRewardAmount`, in cents.
  `GET /core/v4/referrals?Offset&Limit`: `Referrals`, each with `Email`, `CreateTime` and `State`, from `0` invited to
  `4` rewarded. The link is `UserSettings.Referral.Link` of `GET /core/v4/settings`.
- **SMTP tokens.** `GET /mail/v4/smtptokens`: `SmtpTokens`, each with `SmtpTokenID`, `Name`, `AddressID`, `CreateTime` and
  `LastUsedTime`. `POST /mail/v4/smtptokens` with `AddressID` and `Name` answers `SmtpTokenCode`, shown this once.
  `DELETE /mail/v4/smtptokens/{id}`. The server is `smtp.protonmail.ch`, port 587, as in `SMTPTokenModal.tsx`.
- **Users.** `GET /core/v4/organizations`: `Organization` with `Name` and `Used`/`Max` of `Members`, `Addresses`, `Domains`
  and `Space`. `GET /core/v4/members?Page&PageSize`: `Members`, each with `Name`, `Role` (`2` is admin), `State` (`0`
  disabled, `1` enabled, `2` invited), `Self`, `UsedSpace` and `MaxSpace`.

- **Settings of the mail.** Each is `PUT /mail/v4/settings/{path}` with one field, read from `MailSettings`. The table is
  `MAIL_CHOICES` in `mapping/settings.rs`, with values from `packages/shared/lib/mail/mailSettings.ts`. The draft type is
  sent as `MIMEType` and read as `DraftMIMEType`. `SpamAction` and `BlockSenderConfirmation` go out as `null` when unset.
- **Settings of the account.** `PUT /core/v4/settings/telemetry`, `crashreports` and `earlyaccess`, and `invoicetext`.
  `PATCH /core/v4/settings/news` takes names like `{"Offers": true}`, and `News` in `UserSettings` holds them as bits.
  `POST` and `DELETE /core/v4/settings/breachalerts`.

- **What needs the password.** `POST /core/v4/auth/info` with `Intent` and `ReauthScope: password` gives the SRP inputs.
  `PUT /core/v4/users/password` takes `ClientProof`, `ClientEphemeral`, `SRPSession` and `TwoFactorCode`, answers
  `ServerProof`, and opens the scope. Then `PUT /core/v4/settings/email` with `Email`, `/phone` with `Phone`, `/email/reset`
  and `/phone/reset` with `Reset`, `POST` or `DELETE /core/v4/settings/highsecurity`. `PUT /core/v4/users/lock` closes it.
  `PUT /core/v4/settings/2fa/totp` turns the app off and carries the proof itself. To turn it on, `GET .../2fa/totp/secret`
  answers `Secret`, and `POST .../2fa/totp` with `TOTPConfirmation` answers `TwoFactorRecoveryCodes`.

Seen on a free test account: `PUT /core/v4/settings/earlyaccess` answers 400, code 2000, "invalid client", so
it is not offered. `PUT .../logauth` answers 403, code 9102, until the password is confirmed. `GET /core/v4/organizations`
refuses with 422, code 2501, and `GET /core/v4/members` answers 500 without an organization, so it is not asked then.
`GET /core/v4/domains` answers 403, code 2026.

Left on the site: starting an import, which signs in at Google or Microsoft in a browser (`activation.ts`). The export
is Proton's separate Export Tool, so that tab only leads to it.

## Date and time settings

`GET /core/v4/settings` carries `TimeFormat`, `DateFormat` and `WeekStart` in `UserSettings`. They change with
`PUT /core/v4/settings/timeformat`, `/dateformat` and `/weekstart`, each with its one field, as in
`packages/shared/lib/api/settings.ts` of Proton's web client. `0` means "as the language has it" in all three. Time
format: `1` is 24 hours, `2` is 12. Date format: `1` is DD/MM/YYYY, `2` is MM/DD/YYYY, `3` is YYYY-MM-DD. Week start:
`1` is Monday to `7` Sunday (`UserSettings.ts`). Proton Mail has no time zone of its own, so that one stays on this
computer.
