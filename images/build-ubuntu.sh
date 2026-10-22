#!/bin/sh
# Build a bootable Ubuntu arm64 ext4 image with systemd as PID 1.
#
#   ./images/build-ubuntu.sh                 # 26.04.1, 1 GiB, compressed
#   SIZE=2G ./images/build-ubuntu.sh
#
# The host must be arm64 Linux (or an arm64 container) with root/sudo: the
# base tarball is a foreign rootfs, and installing systemd needs a chroot.
# Building an APK on x86_64 uses the ext4 artifact produced by an arm64 job.
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
as_root() { if [ -n "$SUDO" ]; then "$SUDO" "$@"; else "$@"; fi; }

command -v mke2fs >/dev/null 2>&1 || { echo "mke2fs is required (e2fsprogs)" >&2; exit 2; }
case "$(uname -m)" in
    aarch64|arm64) ;;
    *) echo "systemd installation needs a native arm64 chroot; run on arm64 Linux or use a prebuilt ext4" >&2; exit 2 ;;
esac

mkdir -p "$CACHE" "$BUILD" "$DIST"
if [ ! -f "$CACHE/$BASE" ]; then
    echo "fetching $URL"
    curl -fL --retry 3 --progress-bar -o "$CACHE/$BASE.part" "$URL"
    mv "$CACHE/$BASE.part" "$CACHE/$BASE"
fi
if [ ! -f "$CACHE/SHA256SUMS" ]; then
    curl -fL --retry 3 -o "$CACHE/SHA256SUMS" \
        "https://cdimage.ubuntu.com/ubuntu-base/releases/$SERIES/release/SHA256SUMS"
fi
(cd "$CACHE" && grep " \*\?$BASE\$" SHA256SUMS | sha256sum -c -)

ROOT="$BUILD/rootfs-$VERSION-$ARCH"
rm -rf "$ROOT"
mkdir -p "$ROOT"
as_root tar -xpf "$CACHE/$BASE" -C "$ROOT"

# A chroot needs a few device nodes even without /proc. These are only present
# during the build; mke2fs turns them into ordinary device inodes in the image.
mkdir -p "$ROOT/dev"
for spec in "null 1 3" "zero 1 5" "random 1 8" "urandom 1 9" "tty 5 0" "console 5 1" "ptmx 5 2"; do
    set -- $spec
    [ -e "$ROOT/dev/$1" ] && continue
    # Some containers deny mknod through the device cgroup. apt only needs the
    # names to exist; an empty regular file is enough for the build chroot.
    as_root mknod "$ROOT/dev/$1" c "$2" "$3" 2>/dev/null || as_root touch "$ROOT/dev/$1"
done
as_root cp /etc/resolv.conf "$ROOT/etc/resolv.conf"
printf '#!/bin/sh\nexit 101\n' > "$ROOT/usr/sbin/policy-rc.d"
as_root chmod 755 "$ROOT/usr/sbin/policy-rc.d"

# Install systemd into the chroot. Its postinst calls
# systemd-machine-id-setup, which reopens /etc/machine-id through
# /proc/self/fd; /proc cannot be mounted in the common build container. Seed a
# valid machine-id and neutralise that one command, then configure everything.
printf '0123456789abcdef0123456789abcdef\n' > "$ROOT/etc/machine-id"
as_root chmod 444 "$ROOT/etc/machine-id"
apt_install() {
    as_root chroot "$ROOT" env DEBIAN_FRONTEND=noninteractive \
        apt-get install -y --no-install-recommends "$@"
}

as_root chroot "$ROOT" apt-get update -qq
# The first pass unpacks systemd; its configure step is expected to fail on the
# /proc-dependent machine-id helper.
apt_install systemd >/dev/null 2>&1 || true
as_root sed -i 's|^systemd-machine-id-setup.*|true|' \
    "$ROOT/var/lib/dpkg/info/systemd.postinst"
as_root chroot "$ROOT" env DEBIAN_FRONTEND=noninteractive dpkg --configure -a
apt_install systemd-sysv dbus udev iproute2 iputils-ping curl
as_root chroot "$ROOT" apt-get clean
as_root rm -rf "$ROOT/var/cache/apt"/* "$ROOT/var/lib/apt/lists"/*

# systemd: getty on the UML console, an autologin root shell, a marker service.
as_root tee "$ROOT/etc/hostname" >/dev/null <<'HOST'
drolin
HOST
as_root rm -f "$ROOT/etc/systemd/system/getty.target.wants/getty@tty1.service"
as_root mkdir -p "$ROOT/etc/systemd/system/getty@tty0.service.d"
as_root tee "$ROOT/etc/systemd/system/getty@tty0.service.d/autologin.conf" >/dev/null <<'GETTY'
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin root --noclear %I $TERM
Type=idle
GETTY
as_root ln -sf /usr/lib/systemd/system/getty@.service \
    "$ROOT/etc/systemd/system/getty.target.wants/getty@tty0.service"
as_root ln -sf /usr/lib/systemd/system/multi-user.target \
    "$ROOT/etc/systemd/system/default.target"

# DHCP on the UML vector interface. passt is the DHCP server on the other end.
as_root mkdir -p "$ROOT/etc/systemd/network"
as_root tee "$ROOT/etc/systemd/network/20-vec0.network" >/dev/null <<'NETWORK'
[Match]
Name=vec0

[Network]
DHCP=ipv4
NETWORK
as_root mkdir -p "$ROOT/etc/systemd/system/multi-user.target.wants"
as_root mkdir -p "$ROOT/etc/systemd/system/sockets.target.wants"
as_root ln -sf /usr/lib/systemd/system/systemd-networkd.service \
    "$ROOT/etc/systemd/system/multi-user.target.wants/systemd-networkd.service"
as_root ln -sf /usr/lib/systemd/system/systemd-networkd.socket \
    "$ROOT/etc/systemd/system/sockets.target.wants/systemd-networkd.socket"
as_root tee "$ROOT/etc/resolv.conf" >/dev/null <<'RESOLV'
nameserver 1.1.1.1
RESOLV

# A shell that powers off when it exits. The base image has systemd's
# systemctl, but the wrapper also makes `exit` finish the UML process.
if [ -z "${POWEROFF:-}" ]; then
    TARGET=${TARGET:-aarch64-unknown-linux-musl}
    if command -v cargo >/dev/null 2>&1; then
        RUSTFLAGS="${RUSTFLAGS:-} -C linker=rust-lld" \
            cargo build --manifest-path "$HERE/../Cargo.toml" --release \
            --target "$TARGET" -p drolin-init --bin drolin-poweroff >/dev/null 2>&1 || true
        POWEROFF="$HERE/../target/$TARGET/release/drolin-poweroff"
    fi
fi
if [ -n "${POWEROFF:-}" ] && [ -f "$POWEROFF" ]; then
    as_root install -m 755 "$POWEROFF" "$ROOT/usr/sbin/drolin-poweroff"
    as_root ln -sf drolin-poweroff "$ROOT/usr/sbin/poweroff"
    as_root ln -sf drolin-poweroff "$ROOT/usr/sbin/halt"
fi
as_root tee "$ROOT/usr/local/bin/drolin-shell" >/dev/null <<'SHELL'
#!/bin/sh
/bin/bash -l
exec /usr/sbin/drolin-poweroff
SHELL
as_root chmod 755 "$ROOT/usr/local/bin/drolin-shell"
as_root sed -i 's|^root:x:0:0:root:/root:/bin/bash$|root:x:0:0:root:/root:/usr/local/bin/drolin-shell|' \
    "$ROOT/etc/passwd"

as_root tee "$ROOT/usr/local/bin/drolin-marker" >/dev/null <<'MARKER'
#!/bin/sh
printf 'DROLIN_GUEST_OK\n' > /dev/console
. /etc/os-release
printf 'Ubuntu %s (%s)\n' "$VERSION_ID" "$(uname -m)" > /dev/console
if grep -q 'drolin.poweroff=1' /proc/cmdline 2>/dev/null; then
    systemctl poweroff || /usr/sbin/drolin-poweroff
fi
MARKER
as_root chmod 755 "$ROOT/usr/local/bin/drolin-marker"
as_root tee "$ROOT/etc/systemd/system/drolin-marker.service" >/dev/null <<'UNIT'
[Unit]
Description=Drolin guest marker
After=multi-user.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/drolin-marker

[Install]
WantedBy=multi-user.target
UNIT
as_root mkdir -p "$ROOT/etc/systemd/system/multi-user.target.wants"
as_root ln -sf ../drolin-marker.service \
    "$ROOT/etc/systemd/system/multi-user.target.wants/drolin-marker.service"

# mke2fs -d uses listxattr() on each entry, which follows symlinks. Absolute
# symlinks point at the build host, and dangling ones fail the copy.
as_root python3 - "$ROOT" <<'PYFIX'
import os
import sys
root = sys.argv[1]
converted = removed = 0
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

# The /dev stand-ins stay in the tree; mke2fs uses listxattr() on symlinks such
# as systemd's /dev/null generator mask, so deleting them would make the copy
# fail. devtmpfs is mounted over /dev at boot and hides the stand-ins.

IMAGE="$DIST/ubuntu-$VERSION-$ARCH.ext4"
rm -f "$IMAGE"
# mke2fs -d populates from a directory without mounting anything.
as_root mke2fs -q -t ext4 -F -L drolin-ubuntu -d "$ROOT" "$IMAGE" "$SIZE"
as_root e2fsck -fy "$IMAGE" >/dev/null 2>&1 || true
as_root resize2fs -M "$IMAGE" >/dev/null
as_root chmod 644 "$IMAGE"
gzip -n -9 -c "$IMAGE" > "$IMAGE.gz"
ls -lh "$IMAGE" "$IMAGE.gz"
echo "wrote $IMAGE (and .gz); systemd is /sbin/init"
