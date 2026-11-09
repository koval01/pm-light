#!/usr/bin/env bash
# Packs the programs that the build left as artifacts (.github/workflows/build.yml) for a release: one archive for
# each system and their SHA-256 sums. A file is found by its name wherever the artifact keeps it, since
# upload-artifact keeps paths from the folder its paths share, and exactly one of each must be there.
#
#   scripts/pack-release.sh <tag> <folder of the downloaded artifacts> <folder to write> [--no-macos]
#
# `--no-macos` is for an ordinary build, which makes no macOS bundle: every push checks the packing with it.
set -euo pipefail

tag=$1
built=$2
out=$3
macos=yes
[ "${4:-}" = --no-macos ] && macos=no

# The one file of this name under the system's artifact.
one() {
    local found count
    found=$(find "$built/proton-mail-light-$1" -type f -name "$2" 2>/dev/null)
    count=$(printf '%s' "$found" | grep -c . || true)
    if [ "$count" -ne 1 ]; then
        echo "The $1 artifact holds $count files named $2, not one" >&2
        exit 1
    fi
    printf '%s' "$found"
}

mkdir -p "$out"
out=$(cd "$out" && pwd)
windows=$(one Windows proton-mail-light.exe)
linux=$(one Linux proton-mail-light)
if [ "$macos" = yes ]; then
    bundle=$(one macOS Proton-Mail-Light-macos.zip)
    cp "$bundle" "$out/proton-mail-light-$tag-macos.zip"
fi
(cd "$(dirname "$windows")" && zip -q -9 "$out/proton-mail-light-$tag-windows-x64.zip" proton-mail-light.exe)
chmod +x "$linux"
tar -C "$(dirname "$linux")" -czf "$out/proton-mail-light-$tag-linux-x64.tar.gz" proton-mail-light
(cd "$out" && sha256sum -- * > SHA256SUMS)
cat "$out/SHA256SUMS"
