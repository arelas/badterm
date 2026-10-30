#!/bin/sh
# Build BadTerm.app and a disk image holding it, from a release binary.
# Runs on macOS (it needs codesign and hdiutil).
#
#   packaging/macos/build-dmg.sh target/release/badterm 0.1.6 out.dmg
#
# The app is signed ad hoc only: macOS asks before opening it the first time
# (Control-click > Open), until there is a Developer ID to sign with.
set -eu

BIN=$1
VERSION=$2
OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ICONS=$HERE/../../crates/badterm/assets/icon

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
APP=$WORK/BadTerm/BadTerm.app
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/badterm"
cp "$ICONS/badterm.icns" "$APP/Contents/Resources/badterm.icns"
sed "s/@VERSION@/$VERSION/g" "$HERE/Info.plist" > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict "$APP"

# The usual drag-to-install layout.
ln -s /Applications "$WORK/BadTerm/Applications"
rm -f "$OUT"
hdiutil create -quiet -volname BadTerm -srcfolder "$WORK/BadTerm" -format UDZO -fs HFS+ "$OUT"
