#!/bin/sh
# Build the pinned kernel as a User-Mode Linux binary for this host.
#
# Upstream mainline implements the UML subarch for x86 (and i386). It does not
# implement an arm64 UML subarch, so this script refuses an arm64 host with an
# explanation instead of producing a binary that cannot boot. The rest of
# drolin is architecture-neutral: supply a kernel built for the host arch and
# the engine runs it.
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck disable=SC1091
. "$HERE/latest-stable.env"

if [ "$(uname -s)" != "Linux" ]; then
    echo "the Linux kernel must be built on Linux (found $(uname -s))" >&2
    echo "use a Linux container, a Linux host, or CI" >&2
    exit 2
fi

HOST_ARCH=$(uname -m)
case "$HOST_ARCH" in
    x86_64|amd64)
        SUBARCH=x86_64
        ;;
    i386|i686)
        SUBARCH=i386
        ;;
    aarch64|arm64)
        SUBARCH=arm64
        if [ ! -d "$HERE/src/linux-$KERNEL_VERSION/arch/arm64/um" ]; then
            echo "kernel $KERNEL_VERSION has no arch/arm64/um subarch." >&2
            echo "User-Mode Linux on arm64 is not part of upstream mainline." >&2
            echo "Build on an x86_64 host, or supply an arm64 UML kernel built elsewhere." >&2
            exit 3
        fi
        ;;
    *)
        echo "unsupported host architecture: $HOST_ARCH" >&2
        exit 2
        ;;
esac

"$HERE/fetch.sh"

TREE="$HERE/src/linux-$KERNEL_VERSION"
OUT=${DROLIN_KERNEL_OUT:-$HERE/build}
DIST=${DROLIN_KERNEL_DIST:-$HERE/dist}
JOBS=${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}
MAKE="make -C $TREE O=$OUT ARCH=um SUBARCH=$SUBARCH"

if command -v clang >/dev/null 2>&1 && command -v ld.lld >/dev/null 2>&1; then
    MAKE="$MAKE LLVM=1"
fi

export KBUILD_BUILD_TIMESTAMP=${KBUILD_BUILD_TIMESTAMP:-"Thu Jan  1 00:00:00 UTC 1970"}
export KBUILD_BUILD_USER=${KBUILD_BUILD_USER:-drolin}
export KBUILD_BUILD_HOST=${KBUILD_BUILD_HOST:-drolin}

mkdir -p "$OUT" "$DIST"
# shellcheck disable=SC2086
$MAKE defconfig
# shellcheck disable=SC2086
$MAKE scripts/kconfig/merge_config.sh 2>/dev/null || true
# merge_config.sh is a host script, not a make target; call it directly.
"$TREE/scripts/kconfig/merge_config.sh" -m -O "$OUT" "$OUT/.config" "$HERE/config/minimal.config"
# shellcheck disable=SC2086
$MAKE olddefconfig
# shellcheck disable=SC2086
$MAKE -j"$JOBS"

BIN="$OUT/linux"
if [ ! -x "$BIN" ]; then
    echo "build finished without $BIN" >&2
    exit 4
fi
DEST="$DIST/linux-um-$HOST_ARCH"
cp -f "$BIN" "$DEST"
echo "wrote $DEST"
file "$DEST" 2>/dev/null || true
