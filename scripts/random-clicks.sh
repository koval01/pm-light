#!/usr/bin/env bash
# Real clicks and keys at random places of the app's window, as a restless person would, for the long run on Linux
# (.github/workflows/long-run.yml), beside the actions the app takes itself. Pointer events go through XTEST, the way
# a mouse's do, so the window cannot tell them from a person's. No key with Control, Alt or Super is pressed: those
# reach beyond the window. What would end the run or change the account is refused by the app in a check anyway.
#
#   scripts/random-clicks.sh <minutes> <seed> [record]
#
# DISPLAY names the virtual screen. Prints only how many clicks and keys it gave. A record, when named, gets each click
# and key with its time of day in UTC and its place in the window, to be read beside the trace of connections.
set -euo pipefail

minutes=$1
RANDOM=$2
record=${3:-/dev/null}
keys=(Down Up Down Return Escape Tab space Page_Down Page_Up Home End j k Left Right BackSpace a e s)
clicks=0
pressed=0
end=$((SECONDS + minutes * 60))
while [ "$SECONDS" -lt "$end" ]; do
    window=$(xdotool search --onlyvisible --name "Proton Mail Light" 2>/dev/null | head -n 1 || true)
    if [ -n "$window" ]; then
        eval "$(xdotool getwindowgeometry --shell "$window")"
        if [ $((RANDOM % 3)) -eq 0 ]; then
            key=${keys[RANDOM % ${#keys[@]}]}
            xdotool key --clearmodifiers "$key" && pressed=$((pressed + 1))
            echo "$(date -u +%H:%M:%S.%3N) key $key" >> "$record"
        else
            # shellcheck disable=SC2153 # X, Y, WIDTH and HEIGHT come from getwindowgeometry above
            across=$((RANDOM % WIDTH))
            down=$((RANDOM % HEIGHT))
            xdotool mousemove $((X + across)) $((Y + down)) click 1 && clicks=$((clicks + 1))
            echo "$(date -u +%H:%M:%S.%3N) click $across $down of $WIDTH by $HEIGHT" >> "$record"
        fi
    fi
    sleep "$((RANDOM % 3)).$((RANDOM % 10))"
done
echo "random input: $clicks clicks, $pressed keys"
