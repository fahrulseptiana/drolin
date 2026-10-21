#!/bin/sh
# Build a bootable Ubuntu arm64 ext4 image from the official base tarball.
#
#   ./images/build-ubuntu.sh                 # 26.04.1, 1 GiB, compressed
#   SIZE=2G ./images/build-ubuntu.sh
#
# Needs root (or passwordless sudo) for ownership and mke2fs. Running this in a
# Linux arm64 container is the portable path; it never mounts the image.
set -eu

VERSION=${VERSION:-26.04.1}
SERIES=${SERIES:-26.04}
SIZE=${SIZE:-1G}
ARCH=${ARCH:-arm64}
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
CACHE=${DROLIN_IMAGE_CACHE:-$HERE/cache}
BUILD=${DROLIN_IMAGE_BUILD:-$HERE/build}
DIST=${DROLIN_IMAGE_DIST:-$HERE/dist}
BASE="ubuntu-base-$VERSION-base-$ARCH.tar.gz"
URL="https://cdimage.ubuntu.com/ubuntu-base/releases/$SERIES/release/$BASE"

if [ "$(id -u)" = 0 ]; then SUDO=; else SUDO=sudo; fi
command -v mke2fs >/dev/null 2>&1 || { echo "mke2fs is required (e2fsprogs)" >&2; exit 2; }

mkdir -p "$CACHE" "$BUILD" "$DIST"
if [ ! -f "$CACHE/$BASE" ]; then
    echo "fetching $URL"
    curl -fL --retry 3 --progress-bar -o "$CACHE/$BASE.part" "$URL"
    mv "$CACHE/$BASE.part" "$CACHE/$BASE"
fi
# Ubuntu publishes SHA256SUMS next to the tarball.
if [ ! -f "$CACHE/SHA256SUMS" ]; then
    curl -fL --retry 3 -o "$CACHE/SHA256SUMS" \
        "https://cdimage.ubuntu.com/ubuntu-base/releases/$SERIES/release/SHA256SUMS"
fi
(cd "$CACHE" && grep " \*\?$BASE\$" SHA256SUMS | sha256sum -c -)

ROOT="$BUILD/rootfs-$VERSION-$ARCH"
rm -rf "$ROOT"
mkdir -p "$ROOT"
$SUDO tar -xpf "$CACHE/$BASE" -C "$ROOT"

# mke2fs -d uses listxattr() on each entry, which follows symlinks. An absolute
# symlink then points at the build host, and a dangling one fails the copy.
# Make absolute links relative and drop the package-manager doc links that point
# at packages the base tarball does not install.
$SUDO python3 - "$ROOT" <<'PYFIX'
import os
import sys
root = sys.argv[1]
converted = 0
removed = 0
for dirpath, dirnames, filenames in os.walk(root):
    for name in list(dirnames) + list(filenames):
        path = os.path.join(dirpath, name)
        if not os.path.islink(path):
            continue
        target = os.readlink(path)
        if target.startswith("/"):
            target = os.path.relpath(os.path.join(root, target.lstrip("/")), dirpath)
            os.unlink(path)
            os.symlink(target, path)
            converted += 1
        if not os.path.exists(path):
            os.unlink(path)
            removed += 1
print(f"symlinks: {converted} made relative, {removed} dangling removed")
PYFIX

# A tiny init that mounts the pseudo-filesystems and gives a login shell. It is
# deliberately not systemd: UML has no ACPI, no EFI and no real devices, and a
# shell is enough to call the Ubuntu userland booted.
$SUDO tee "$ROOT/init" >/dev/null <<'INIT'
#!/bin/sh
export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export HOME=/root
mount -t proc proc /proc 2>/dev/null || true
mount -t sysfs sysfs /sys 2>/dev/null || true
mount -t devtmpfs devtmpfs /dev 2>/dev/null || true
mkdir -p /dev/pts /run /tmp
mount -t devpts devpts /dev/pts 2>/dev/null || true
mount -t tmpfs tmpfs /run 2>/dev/null || true
mount -t tmpfs tmpfs /tmp 2>/dev/null || true
hostname drolin
printf 'DROLIN_GUEST_OK\n'
printf 'Ubuntu %s (%s)\n' "$(sed -n 's/^VERSION_ID="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' /etc/os-release)" "$(uname -m)"
case " $(cat /proc/cmdline 2>/dev/null) " in
    *" drolin.poweroff=1 "*)
        if [ -x /usr/sbin/drolin-poweroff ]; then
            exec /usr/sbin/drolin-poweroff
        fi
        poweroff -f 2>/dev/null || reboot -f 2>/dev/null || true
        ;;
esac
if [ -t 0 ]; then
    /bin/bash -l
else
    /bin/bash -l -i
fi
if [ -x /usr/sbin/drolin-poweroff ]; then
    exec /usr/sbin/drolin-poweroff
fi
exit 0
INIT
$SUDO chmod 755 "$ROOT/init"

# Ubuntu base has no systemd, so there is no poweroff/reboot command. Ship the
# tiny syscall wrapper when one is supplied or can be built.
if [ -z "${POWEROFF:-}" ]; then
    TARGET=${TARGET:-}
    if [ -z "$TARGET" ]; then
        case "$(uname -m)" in
            aarch64|arm64) TARGET=aarch64-unknown-linux-musl ;;
            x86_64) TARGET=x86_64-unknown-linux-musl ;;
        esac
    fi
    if [ -n "${TARGET:-}" ] && command -v cargo >/dev/null 2>&1; then
        RUSTFLAGS="${RUSTFLAGS:-} -C linker=rust-lld" \
            cargo build --manifest-path "$HERE/../Cargo.toml" --release \
            --target "$TARGET" -p drolin-init --bin drolin-poweroff >/dev/null 2>&1 || true
        POWEROFF="$HERE/../target/$TARGET/release/drolin-poweroff"
    fi
fi
if [ -n "${POWEROFF:-}" ] && [ -f "$POWEROFF" ]; then
    $SUDO install -m 755 "$POWEROFF" "$ROOT/usr/sbin/drolin-poweroff"
    $SUDO ln -sf drolin-poweroff "$ROOT/usr/sbin/poweroff"
    $SUDO ln -sf drolin-poweroff "$ROOT/usr/sbin/halt"
fi

IMAGE="$DIST/ubuntu-$VERSION-$ARCH.ext4"
rm -f "$IMAGE"
# mke2fs -d populates from a directory without mounting anything.
$SUDO mke2fs -q -t ext4 -F -L drolin-ubuntu -d "$ROOT" "$IMAGE" "$SIZE"
$SUDO e2fsck -fy "$IMAGE" >/dev/null 2>&1 || true
$SUDO resize2fs -M "$IMAGE" >/dev/null
$SUDO chmod 644 "$IMAGE"
gzip -n -9 -c "$IMAGE" > "$IMAGE.gz"
ls -lh "$IMAGE" "$IMAGE.gz"
echo "wrote $IMAGE (and .gz)"
