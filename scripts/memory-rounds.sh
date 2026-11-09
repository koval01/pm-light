#!/usr/bin/env bash
# Shows what the account check's memory rounds measured (app/src/memory_check.rs) and warns when the app kept
# growing after the first round, which fills what is meant to be filled. Numbers only.
#
#   scripts/memory-rounds.sh <file>
set -euo pipefail

# More than this after the first round, over five rounds of the whole inbox, is worth a look.
most=40
file=$1
if [ ! -f "$file" ]; then
    echo "no memory rounds were measured"
    exit 0
fi
cat "$file"
grew=$(sed -n 's/^grew MB=//p' "$file")
if [ "${grew:-0}" -gt "$most" ]; then
    echo "::warning::The app grew by $grew MB over its memory rounds, more than $most MB"
fi
