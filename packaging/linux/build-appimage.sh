#!/bin/sh
# Build Phosphor.AppImage from a release binary.
#
#   packaging/linux/build-appimage.sh target/release/phosphor 0.1.6 out.AppImage
#
# appimagetool and the AppImage runtime are downloaded once into a cache
# directory and checked against pinned SHA-256 sums (the digests GitHub
# publishes for these release files), so a changed download fails the build.
set -eu

BIN=$1
VERSION=$2
OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ICONS=$HERE/../../crates/phosphor/assets/icon
CACHE=${PHOSPHOR_TOOL_CACHE:-$HOME/.cache/phosphor-packaging}

TOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
TOOL_SHA=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
RUNTIME_SHA=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

fetch() { # url sha file
    if [ -f "$3" ] && echo "$2  $3" | sha256sum -c --status; then
        return
    fi
    curl --proto '=https' --tlsv1.2 -sSfLo "$3.part" "$1"
    echo "$2  $3.part" | sha256sum -c --quiet
    mv "$3.part" "$3"
}

mkdir -p "$CACHE"
fetch "$TOOL_URL" "$TOOL_SHA" "$CACHE/appimagetool-1.9.1"
fetch "$RUNTIME_URL" "$RUNTIME_SHA" "$CACHE/runtime-20251108"
chmod +x "$CACHE/appimagetool-1.9.1"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
APPDIR=$WORK/Phosphor.AppDir
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps"
cp "$BIN" "$APPDIR/usr/bin/phosphor"
cp "$HERE/phosphor.desktop" "$APPDIR/phosphor.desktop"
cp "$HERE/phosphor.desktop" "$APPDIR/usr/share/applications/phosphor.desktop"
cp "$ICONS/phosphor-256.png" "$APPDIR/phosphor.png"
cp "$ICONS/phosphor-256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/phosphor.png"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/usr/bin/phosphor" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

# --appimage-extract-and-run: no FUSE needed on the build machine.
ARCH=x86_64 VERSION=$VERSION "$CACHE/appimagetool-1.9.1" --appimage-extract-and-run --no-appstream \
    --runtime-file "$CACHE/runtime-20251108" "$APPDIR" "$OUT"
