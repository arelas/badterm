#!/bin/sh
# Build the Linux release binary against glibc 2.28, so the packages run on
# Debian 10, Ubuntu 20.04, RHEL 8 and anything newer, whatever glibc the
# build machine has. cargo-zigbuild links with Zig's toolchain, which
# carries the old glibc's symbol versions.
#
#   packaging/linux/build-release.sh    # -> target/x86_64-unknown-linux-gnu/release/phosphor
#
# Zig is downloaded once into a cache directory and checked against the
# SHA-256 sum ziglang.org publishes for it; cargo-zigbuild is installed
# there at a pinned version.
set -eu

CACHE=${PHOSPHOR_TOOL_CACHE:-$HOME/.cache/phosphor-packaging}
GLIBC=2.28
TARGET=x86_64-unknown-linux-gnu

ZIG_VERSION=0.16.0
ZIG_URL=https://ziglang.org/download/$ZIG_VERSION/zig-x86_64-linux-$ZIG_VERSION.tar.xz
ZIG_SHA=70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00
ZIGBUILD_VERSION=0.23.4

mkdir -p "$CACHE"
ZIG_DIR=$CACHE/zig-x86_64-linux-$ZIG_VERSION
if [ ! -x "$ZIG_DIR/zig" ]; then
    curl --proto '=https' --tlsv1.2 -sSfLo "$CACHE/zig.tar.xz.part" "$ZIG_URL"
    echo "$ZIG_SHA  $CACHE/zig.tar.xz.part" | sha256sum -c --quiet
    tar -xJf "$CACHE/zig.tar.xz.part" -C "$CACHE"
    rm "$CACHE/zig.tar.xz.part"
fi
ZIGBUILD_ROOT=$CACHE/cargo-zigbuild-$ZIGBUILD_VERSION
if [ ! -x "$ZIGBUILD_ROOT/bin/cargo-zigbuild" ]; then
    cargo install --quiet --locked --version "$ZIGBUILD_VERSION" --root "$ZIGBUILD_ROOT" cargo-zigbuild
fi

rustup target add "$TARGET" >/dev/null 2>&1 || true
export PATH="$ZIG_DIR:$ZIGBUILD_ROOT/bin:$PATH"
cargo zigbuild --release --locked --target "$TARGET.$GLIBC" -p phosphor

BIN=target/$TARGET/release/phosphor
NEWEST=$(objdump -T "$BIN" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -1)
echo "$BIN needs glibc $NEWEST"
if [ "$(printf '%s\n%s\n' "$NEWEST" "$GLIBC" | sort -V | tail -1)" != "$GLIBC" ]; then
    echo "error: $BIN needs glibc $NEWEST, newer than $GLIBC" >&2
    exit 1
fi
