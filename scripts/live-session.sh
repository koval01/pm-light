#!/usr/bin/env bash
# Hands the session this Mac's app saved to GitHub, for the account check in .github/workflows/live.yml. Run by the
# maintainer on their own Mac; macOS asks before it lets the session out of the keychain. The session goes straight
# from the keychain into GitHub's secret store and is never shown or written anywhere.
#
# The first run also makes the environment `live-account`, whose secret only a run the maintainer approved can read,
# and only from main. Run it again whenever the check says the session needs to be handed over anew.
set -euo pipefail

repo=koval01/pm-light
service=org.koval-dev.proton-mail-light

me=$(gh api user -q .id)
gh api -X PUT "repos/$repo/environments/live-account" --input - >/dev/null <<JSON
{
  "wait_timer": 0,
  "reviewers": [{ "type": "User", "id": $me }],
  "deployment_branch_policy": { "protected_branches": true, "custom_branch_policies": false }
}
JSON
echo "the environment live-account waits for your approval on every run"

session=$(security find-generic-password -s "$service" -a session -w)
if printf '%s' "$session" | grep -q '"sealed"'; then
    echo "this session is sealed with the app password: the check could not open it" >&2
    exit 1
fi
printf '%s' "$session" | gh secret set LIVE_SESSION --env live-account --repo "$repo"
unset session
echo "the session is in GitHub: run the check from Actions, Live account, Run workflow"
