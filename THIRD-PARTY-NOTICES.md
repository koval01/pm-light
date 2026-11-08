# Third-party notices

## Proton

The protocol work follows Proton's own open-source code, which is the reference for this client:

| Upstream                                                                | License | Used for                                                                                  |
| ----------------------------------------------------------------------- | ------- | ----------------------------------------------------------------------------------------- |
| [ProtonMail/go-srp](https://github.com/ProtonMail/go-srp)               | MIT     | SRP handshake, `expandHash`, password and key derivation, the modulus signing key         |
| [ProtonMail/go-proton-api](https://github.com/ProtonMail/go-proton-api) | MIT     | Endpoints, payload shapes, send schemes, key flags, retry behaviour                       |
| [ProtonMail/WebClients](https://github.com/ProtonMail/WebClients)       | GPL-3.0 | Read as a specification of the API (package layout, signature flags). No code was copied. |

MIT License, Copyright (c) 2019 Proton Technologies AG.

"Proton" and "Proton Mail" are trademarks of Proton AG. This project is not affiliated with Proton AG.

## protonlite

[Maytrella007/protonlite](https://github.com/Maytrella007/protonlite) showed that a third-party client can identify itself
as `Other`. The first mail cleaner and the send-crypto approach came from it, and a signed modulus used in early tests.
The cleaner has since been rewritten in Rust (`core/src/html.rs`), and the fixture went with the TypeScript code.

```
MIT License

Copyright (c) 2026 William Wilamo

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## pm-srp test vectors

`core/src/crypto/test_vectors.json` holds test data from
[ProtonMail/pm-srp](https://github.com/ProtonMail/pm-srp), MIT License, Copyright (c) Proton Technologies AG.
It is used by the tests only and is not part of the built client.

## Rust

Cryptography: [ProtonMail/proton-crypto-rs](https://github.com/ProtonMail/proton-crypto-rs), MIT License,
Copyright (c) Proton AG. Window: [Slint](https://slint.dev) under the GNU GPL, version 3, which section 13 of the
AGPL allows this program to be combined with, with
[winit](https://github.com/rust-windowing/winit) and [softbuffer](https://github.com/rust-windowing/softbuffer)
(patched copies in `vendor`); webviews: [wry](https://github.com/tauri-apps/wry), MIT or Apache-2.0. HTML cleaning:
[ammonia](https://github.com/rust-ammonia/ammonia) and [lol_html](https://github.com/cloudflare/lol-html).

Every component of the built program, with its version, license and the texts of its license files, is listed in
`app/components/` by `scripts/components.py`.
