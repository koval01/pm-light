#!/usr/bin/env bash
# What the app said during a check, for the run's page: its own journal's lines, without the system's noise around
# them (D-Bus, portals, GTK), and a plain word when the session handed to CI has expired, which a check may not renew.
# The journal holds numbers and fixed words only (app/tests/journal_privacy.rs).
#
#   scripts/check-log.sh <log> [lines]
set -euo pipefail

log=$1
lines=${2:-60}
[ -f "$log" ] || { echo "no log at $log"; exit 0; }
grep -E "^([0-9:.]+ )?(INFO|WARN|ERROR|DEBUG) " "$log" | cut -c1-200 | tail -n "$lines" || true
if grep -q "Proton answered 401" "$log"; then
    echo "::error::Proton refused the session handed to CI: it has expired, and a check may not renew it. Hand it over again with scripts/live-session.sh on the Mac where the app is signed in."
fi
