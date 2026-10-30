#!/bin/sh
# Build Phosphor.app and a disk image holding it, from a release binary.
# Runs on macOS (it needs codesign and hdiutil).
#
#   packaging/macos/build-dmg.sh target/release/phosphor 0.1.6 out.dmg
#
# The app is signed ad hoc only: macOS asks before opening it the first time
# (Control-click > Open), until there is a Developer ID to sign with.
set -eu

BIN=$1
VERSION=$2
OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ICONS=$HERE/../../crates/phosphor/assets/icon

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
APP=$WORK/Phosphor/Phosphor.app
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/phosphor"
cp "$ICONS/phosphor.icns" "$APP/Contents/Resources/phosphor.icns"
sed "s/@VERSION@/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict "$APP"

# The usual drag-to-install layout.
ln -s /Applications "$WORK/Phosphor/Applications"
rm -f "$OUT"
hdiutil create -quiet -volname Phosphor -srcfolder "$WORK/Phosphor" -format UDZO -fs HFS+ "$OUT"
