#!/usr/bin/env bash
# Checks, builds and puts the program into an app bundle. macOS takes the name in the menu bar and
# the icon in the Dock from there: a bare binary is shown under its file name, with no icon.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
"$root/scripts/check.sh"
cargo build --profile native --locked -p proton-mail-light --manifest-path "$root/Cargo.toml"

target="$root/target/native"

# Only the system's own libraries may be linked: anything else is on no one else's Mac.
foreign="$(otool -L "$target/proton-mail-light" | tail -n +2 | awk '{print $1}' | grep -v -e '^/System/' -e '^/usr/lib/' || true)"
if [ -n "$foreign" ]; then
    echo "The program links libraries that are not the system's:" >&2
    echo "$foreign" >&2
    exit 1
fi
app="$target/Proton Mail Light.app"
version="$(awk -F'"' '/^version/ { print $2; exit }' "$root/app/Cargo.toml")"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target/proton-mail-light" "$app/Contents/MacOS/proton-mail-light"
cp "$root/app/icons/icon.icns" "$app/Contents/Resources/icon.icns"
# The license, its additional terms and the components' own licenses travel with the program, as they require.
cp "$root/LICENSE" "$root/NOTICE" "$app/Contents/Resources/"
cp "$root/app/components/notices.txt" "$app/Contents/Resources/THIRD-PARTY-LICENSES.txt"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleDisplayName</key><string>Proton Mail Light</string>
    <key>CFBundleExecutable</key><string>proton-mail-light</string>
    <key>CFBundleIconFile</key><string>icon</string>
    <key>CFBundleIdentifier</key><string>org.koval-dev.proton-mail-light</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>Proton Mail Light</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleURLTypes</key>
    <array>
      <dict>
        <key>CFBundleTypeRole</key><string>Editor</string>
        <key>CFBundleURLName</key><string>org.koval-dev.proton-mail-light.mailto</string>
        <key>CFBundleURLSchemes</key><array><string>mailto</string></array>
      </dict>
    </array>
    <key>CFBundleShortVersionString</key><string>$version</string>
    <key>CFBundleVersion</key><string>$version</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
    <key>LSMinimumSystemVersion</key><string>13.3</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSHumanReadableCopyright</key><string>Unofficial client. Not affiliated with Proton AG.</string>
</dict>
</plist>
PLIST

"$root/scripts/sign-local.sh" "$app"
