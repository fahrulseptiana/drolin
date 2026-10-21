#!/bin/sh
# Fetch and verify the pinned stable kernel. Re-running is cheap: an existing
# extracted tree with a matching version file is left alone.
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck disable=SC1091
. "$HERE/latest-stable.env"

SRC=${DROLIN_KERNEL_SRC:-$HERE/src}
CACHE=${DROLIN_KERNEL_CACHE:-$HERE/cache}
TARBALL="$CACHE/linux-$KERNEL_VERSION.tar.xz"
TREE="$SRC/linux-$KERNEL_VERSION"
STAMP="$TREE/.drolin-version"

if [ -f "$STAMP" ] && [ "$(cat "$STAMP")" = "$KERNEL_VERSION" ]; then
    echo "kernel $KERNEL_VERSION already in $TREE"
    exit 0
fi

mkdir -p "$CACHE" "$SRC"
if [ ! -f "$TARBALL" ]; then
    URL="https://cdn.kernel.org/pub/linux/kernel/$KERNEL_SERIES/linux-$KERNEL_VERSION.tar.xz"
    echo "fetching $URL"
    curl -fL --retry 3 --progress-bar -o "$TARBALL.part" "$URL"
    mv "$TARBALL.part" "$TARBALL"
fi

if command -v sha256sum >/dev/null 2>&1; then
    echo "$KERNEL_SHA256  $TARBALL" | sha256sum -c -
else
    echo "$KERNEL_SHA256  $TARBALL" | shasum -a 256 -c -
fi

rm -rf "$TREE"
tar -C "$SRC" -xf "$TARBALL"
printf '%s\n' "$KERNEL_VERSION" > "$STAMP"
echo "kernel $KERNEL_VERSION ready in $TREE"
