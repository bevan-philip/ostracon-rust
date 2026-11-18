#!/usr/bin/env bash
# Builds dist/Ostracon.app: the app, with the CLI (as ostracon-cli) and ostracon-mcp beside it in Contents/MacOS.
#
#   scripts/bundle-macos.sh           # release build
#   scripts/bundle-macos.sh --open    # and launch it
#
# Notifications only work from the bundle, not from `cargo run`. The bundle is ad-hoc signed. Built locally
# it carries no quarantine flag, so Gatekeeper doesn't stand in the way on a managed machine.
set -euo pipefail

cd "$(dirname "$0")/.."
APP="dist/Ostracon.app"
VERSION="${VERSION:-$(cargo metadata --no-deps --format-version 1 | sed -E 's/.*"name":"ostracon-app","version":"([^"]+)".*/\1/')}"
BUILD_NUMBER="${BUILD_NUMBER:-1}"

cargo build --release -p ostracon-app -p ostracon-cli -p ostracon-mcp

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/ostracon-app "$APP/Contents/MacOS/Ostracon"
# The filesystem is usually case-insensitive, so the CLI can't be called "ostracon" beside "Ostracon".
cp target/release/ostracon "$APP/Contents/MacOS/ostracon-cli"
cp target/release/ostracon-mcp "$APP/Contents/MacOS/ostracon-mcp"

ICONSET="$(mktemp -d)/Ostracon.iconset"
swift scripts/make-icon.swift "$ICONSET"
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/Ostracon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Ostracon</string>
    <key>CFBundleDisplayName</key><string>Ostracon</string>
    <key>CFBundleIdentifier</key><string>uk.bphilip.ostracon.app</string>
    <key>CFBundleExecutable</key><string>Ostracon</string>
    <key>CFBundleIconFile</key><string>Ostracon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$BUILD_NUMBER</string>
    <key>LSMinimumSystemVersion</key><string>13.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST

codesign --force --sign - "$APP/Contents/MacOS/ostracon-cli"
codesign --force --sign - "$APP/Contents/MacOS/ostracon-mcp"
codesign --force --sign - "$APP"
echo "Built $APP"

if [[ "${1:-}" == "--open" ]]; then
    open "$APP"
fi
