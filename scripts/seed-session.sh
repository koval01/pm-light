#!/usr/bin/env bash
# Puts the test account's session, from LIVE_SESSION, where the app keeps one, for the checks in
# .github/workflows/live.yml and long-run.yml. On a CI machine only, which is thrown away after the run.
#
# On macOS a keychain item answers only the program that made it, and the app's question for the login keychain's
# password, which no one there knows, would hold up the check. So the session goes into a keychain of the run's own,
# with a password made for it, and its partition list lets the unsigned app read it. Elsewhere the app's own store
# takes it through the seed_session example; on Linux, inside the D-Bus session the app runs in.
set -euo pipefail

: "${LIVE_SESSION:?the session to put in place}"
app=${1:-target/debug/proton-mail-light}
if [ "$(uname -s)" = Darwin ]; then
    keychain="${RUNNER_TEMP:?the runner gives a folder of its own}/live.keychain-db"
    password=$(openssl rand -hex 24)
    security create-keychain -p "$password" "$keychain"
    security set-keychain-settings "$keychain"
    security unlock-keychain -p "$password" "$keychain"
    # shellcheck disable=SC2046 # the list of keychains is split on purpose
    security list-keychains -d user -s "$keychain" $(security list-keychains -d user | tr -d '"')
    security default-keychain -d user -s "$keychain"
    security add-generic-password -U -A -s org.koval-dev.proton-mail-light -a session -w "$LIVE_SESSION" "$keychain"
    cdhash=$(codesign -dvvv "$app" 2>&1 | sed -n 's/^CDHash=//p' | head -n 1)
    security set-generic-password-partition-list -S "apple-tool:,apple:,unsigned:${cdhash:+,cdhash:$cdhash}" \
        -s org.koval-dev.proton-mail-light -a session -k "$password" "$keychain" > /dev/null
    unset password
    echo "the session is in a keychain of the run's own"
else
    cargo run --locked -q -p proton-mail-light --example seed_session
fi
