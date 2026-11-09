#!/usr/bin/env bash
# Photographs the whole screen at each step of the app's screen tour (app/src/screen_tour.rs), for the account check
# in .github/workflows/live.yml. The app writes a step's name to <note>.step once it has settled; this takes the
# picture under that name, removes the file, and the app goes on. <note>.done, or ten minutes, ends it.
#
#   scripts/photograph-tour.sh <note> <folder for the pictures>
#
# On Linux the screen is the virtual one in DISPLAY.
set -euo pipefail

note=$1
folder=$2
mkdir -p "$folder"

shoot() {
    case "$(uname -s)" in
        Darwin) screencapture -x "$1" ;;
        Linux) import -display "${DISPLAY:?}" -window root "$1" ;;
        *)
            powershell -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms, System.Drawing;
                \$s=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds;
                \$b=New-Object System.Drawing.Bitmap \$s.Width, \$s.Height;
                [System.Drawing.Graphics]::FromImage(\$b).CopyFromScreen(\$s.Location, [System.Drawing.Point]::Empty, \$s.Size);
                \$b.Save('$1')"
            ;;
    esac
}

# Windows: how many Edge processes run at each step, since the runner's Edge has stood in front of the window.
edge() {
    case "$(uname -s)" in
        Darwin | Linux) ;;
        *) echo "$name: Edge processes $(powershell -NoProfile -Command '(Get-Process msedge -ErrorAction SilentlyContinue | Measure-Object).Count')" ;;
    esac
}

taken=0
end=$((SECONDS + 600))
while [ "$SECONDS" -lt "$end" ] && [ ! -f "$note.done" ]; do
    if [ -f "$note.step" ]; then
        name=$(head -c 64 "$note.step")
        # The name becomes a file name: only what the app writes is taken.
        case "$name" in
            '' | *[!a-z0-9-]*) echo "a step with an unexpected name was skipped" ;;
            *)
                # The step's file is written once the app is done with it; a moment more for the screen to show it.
                sleep 1
                if shoot "$folder/$name.png"; then taken=$((taken + 1)); else echo "no picture of $name"; fi
                edge
                ;;
        esac
        rm -f "$note.step"
    fi
    sleep 0.3
done
echo "the screen tour: $taken pictures"
[ -f "$note.done" ] || echo "the screen tour did not finish in time"
