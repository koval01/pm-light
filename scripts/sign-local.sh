#!/usr/bin/env bash
# Signs a build for this computer. The Keychain lets a program back in by its Team ID, and by the
# exact hash of the build when it has none, which is why an unsigned or self-signed build is asked
# again after every compile. An "Apple Development" certificate carries a Team ID, and a free Apple
# ID added in Xcode gets one. It proves nothing to anyone else: a public release needs a Developer ID.
set -euo pipefail

identifier="org.koval-dev.proton-mail-light"
binary="${1:?usage: sign-local.sh path/to/binary}"

# By hash: an expired certificate with the same name would make the name ambiguous.
identity="$(security find-identity -v -p codesigning | awk '/Apple Development|Developer ID Application/ { print $2; exit }')"
if [ -z "$identity" ]; then
    echo "No Apple Development certificate: macOS will ask for Keychain access after every build." >&2
    echo "Add an Apple ID in Xcode, Settings, Accounts, then Manage Certificates." >&2
    # Signed for no one, which still seals the bundle as a whole, so macOS will start it.
    codesign --force --sign - --identifier "$identifier" "$binary"
    exit 0
fi
codesign --force --sign "$identity" --identifier "$identifier" --options runtime --timestamp=none "$binary"
