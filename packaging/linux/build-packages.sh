#!/bin/sh
# Build BadTerm's .deb and .rpm from a release binary.
#
#   packaging/linux/build-packages.sh target/release/badterm 0.1.7 out-prefix
#
# writes out-prefix.deb and out-prefix.rpm. nfpm is downloaded once into a
# cache directory and checked against a pinned SHA-256 sum (the one in the
# release's checksums.txt, which matches GitHub's digest for the file).
set -eu

BIN=$1
VERSION=$2
OUT=$3
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$HERE/../..
CACHE=${BADTERM_TOOL_CACHE:-$HOME/.cache/badterm-packaging}

NFPM_VERSION=2.48.0
NFPM_URL=https://github.com/goreleaser/nfpm/releases/download/v$NFPM_VERSION/nfpm_${NFPM_VERSION}_Linux_x86_64.tar.gz
NFPM_SHA=6d7f9a28b2f397934403950fcbba83f598b08d02edbe79a0815b3b4897341750

mkdir -p "$CACHE"
TARBALL=$CACHE/nfpm-$NFPM_VERSION.tar.gz
if ! { [ -f "$TARBALL" ] && echo "$NFPM_SHA  $TARBALL" | sha256sum -c --status; }; then
    curl --proto '=https' --tlsv1.2 -sSfLo "$TARBALL.part" "$NFPM_URL"
    echo "$NFPM_SHA  $TARBALL.part" | sha256sum -c --quiet
    mv "$TARBALL.part" "$TARBALL"
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
tar -xzf "$TARBALL" -C "$WORK" nfpm

STAGE=$WORK/stage
mkdir -p "$STAGE/icons"
cp "$BIN" "$STAGE/badterm"
cp "$HERE/badterm.desktop" "$ROOT/README.md" "$ROOT/LICENSE" "$STAGE/"
SIZES="16 24 32 48 64 128 256 512"
for size in $SIZES; do
    mkdir -p "$STAGE/icons/${size}x$size/apps"
    cp "$ROOT/crates/badterm/assets/icon/badterm-$size.png" "$STAGE/icons/${size}x$size/apps/badterm.png"
done

chmod -R u=rwX,go=rX "$STAGE"

# The newest glibc symbol version the binary uses is the oldest glibc it
# runs on.
GLIBC=$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
echo "Needs glibc $GLIBC"

# nfpm expands variables in only some fields, so fill them in here.
sed -e "s|\${VERSION}|$VERSION|g" -e "s|\${STAGE}|$STAGE|g" -e "s|\${GLIBC}|$GLIBC|g" \
    "$HERE/nfpm.yaml" > "$WORK/nfpm.yaml"
for size in $SIZES; do
    d=${size}x$size/apps/badterm.png
    printf '  - src: %s\n    dst: %s\n' "$STAGE/icons/$d" "/usr/share/icons/hicolor/$d" >> "$WORK/nfpm.yaml"
done
for fmt in deb rpm; do
    "$WORK/nfpm" package --config "$WORK/nfpm.yaml" --packager "$fmt" --target "$OUT.$fmt"
done
