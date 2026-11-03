#!/bin/sh
# Build BadTerm.app and a disk image holding it, from a release binary.
# Runs on macOS (it needs codesign and hdiutil, and for notarization xcrun
# notarytool/stapler and spctl).
#
#   packaging/macos/build-dmg.sh target/release/badterm 0.1.6 out.dmg
#
# Signing, from the environment (CI's "Set up signing keychain" step sets
# these from the account's secrets, as ZephyrFTP's build does):
#   SIGN_IDENTITY, SIGN_KEYCHAIN
#       a "Developer ID Application" identity and the keychain holding it.
#       Set: the app is signed with it, with the hardened runtime and a
#       secure timestamp, and so is the disk image. Unset: the app is
#       signed ad hoc only, and macOS asks before opening it the first time.
#   NOTARY_KEY_FILE, NOTARY_KEY_ID, NOTARY_ISSUER_ID
#       an App Store Connect API key. Set (releases): the disk image is
#       notarized by Apple, the ticket stapled to it, and Gatekeeper asked
#       what it thinks of it; a rejection fails the build.
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

if [ -n "${SIGN_IDENTITY:-}" ]; then
    : "${SIGN_KEYCHAIN:?SIGN_KEYCHAIN is not set}"
    # One binary and no frameworks, so signing the bundle signs everything.
    codesign --force --timestamp --options runtime \
        --keychain "$SIGN_KEYCHAIN" --sign "$SIGN_IDENTITY" "$APP"
    codesign --display --verbose=2 "$APP" 2>&1 | grep -E '^(Authority|TeamIdentifier|Runtime|Timestamp)' || true
else
    echo "No signing identity: the app is signed ad hoc"
    codesign --force --sign - --timestamp=none "$APP"
fi
codesign --verify --strict --verbose=2 "$APP"

# The usual drag-to-install layout.
ln -s /Applications "$WORK/BadTerm/Applications"
rm -f "$OUT"
hdiutil create -quiet -volname BadTerm -srcfolder "$WORK/BadTerm" -format UDZO -fs HFS+ "$OUT"

[ -n "${SIGN_IDENTITY:-}" ] || exit 0
codesign --force --timestamp --keychain "$SIGN_KEYCHAIN" --sign "$SIGN_IDENTITY" "$OUT"
codesign --verify --verbose=2 "$OUT"
[ -n "${NOTARY_KEY_FILE:-}" ] || { echo "Signed, not notarized (not a release)"; exit 0; }

# --wait returns once Apple has a verdict; the status field says which.
xcrun notarytool submit "$OUT" --key "$NOTARY_KEY_FILE" --key-id "$NOTARY_KEY_ID" \
    --issuer "$NOTARY_ISSUER_ID" --wait --timeout 30m --output-format json > "$WORK/notary.json"
cat "$WORK/notary.json"; echo
status=$(plutil -extract status raw -o - "$WORK/notary.json")
if [ "$status" != "Accepted" ]; then
    id=$(plutil -extract id raw -o - "$WORK/notary.json")
    xcrun notarytool log "$id" --key "$NOTARY_KEY_FILE" --key-id "$NOTARY_KEY_ID" \
        --issuer "$NOTARY_ISSUER_ID" || true
    echo "error: notarization $status" >&2
    exit 1
fi
xcrun stapler staple "$OUT"
xcrun stapler validate "$OUT"
# What Gatekeeper will say to someone opening the downloaded image.
spctl --assess --type open --context context:primary-signature --verbose=2 "$OUT"
