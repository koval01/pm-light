#!/usr/bin/env bash
# Draws the logo's pictures and the app's icons from their sources in app/ui/logo/source.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
source="$root/app/ui/logo/source"
parts="$root/app/ui/logo"
icons="$root/app/icons"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

# The splash's envelope is drawn by app/src/splash.rs, frame by frame at the screen's pixels: only the finished logo
# is a picture.
rsvg-convert -w 352 -h 352 "$source/logo.svg" -o "$parts/logo.png"

rsvg-convert -w 44 -h 44 "$source/tray.svg" -o "$icons/tray.png"
for file in "$icons"/*.png; do
    name="$(basename "$file")"
    [ "$name" = "tray.png" ] && continue
    size="$(sips -g pixelWidth "$file" | awk '/pixelWidth/ { print $2 }')"
    rsvg-convert -w "$size" -h "$size" "$source/icon.svg" -o "$file"
done

set_dir="$scratch/icon.iconset"
mkdir "$set_dir"
for size in 16 32 64 128 256 512; do
    rsvg-convert -w "$size" -h "$size" "$source/icon.svg" -o "$set_dir/icon_${size}x${size}.png"
    rsvg-convert -w $((size * 2)) -h $((size * 2)) "$source/icon.svg" -o "$set_dir/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$set_dir" -o "$icons/icon.icns"
magick "$set_dir/icon_16x16.png" "$set_dir/icon_32x32.png" "$set_dir/icon_64x64.png" "$set_dir/icon_256x256.png" "$icons/icon.ico"
