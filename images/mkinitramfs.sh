#!/bin/sh
# Build the demonstration guest and pack it as a gzipped newc initramfs.
#
#   ./images/mkinitramfs.sh
#   TARGET=aarch64-unknown-linux-musl INIT=path/to/init ./images/mkinitramfs.sh
#
# The default target matches the build host. Building a static Linux binary on
# macOS needs a Linux linker; in CI and on a Linux host rustup's musl target is
# enough (`rustup target add <target>`).
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$HERE/.." && pwd)
TARGET=${TARGET:-}
INIT=${INIT:-}
OUT=${OUT:-$HERE/dist/initramfs.cpio.gz}

if [ -z "$TARGET" ]; then
    case "$(uname -m)" in
        x86_64) TARGET=x86_64-unknown-linux-musl ;;
        aarch64|arm64) TARGET=aarch64-unknown-linux-musl ;;
        *) echo "unsupported build host: $(uname -m)" >&2; exit 2 ;;
    esac
fi

if [ -z "$INIT" ]; then
    echo "building drolin-init for $TARGET"
    # rust-lld is self-contained for the musl targets, including when the build
    # host is macOS. A Linux host can use it too; no external cross linker is
    # required either way.
    RUSTFLAGS="${RUSTFLAGS:-} -C linker=rust-lld" \
        cargo build --manifest-path "$ROOT/Cargo.toml" --release --target "$TARGET" -p drolin-init
    INIT="$ROOT/target/$TARGET/release/drolin-init"
fi

mkdir -p "$HERE/dist"
mkdir -p "$(dirname -- "$OUT")"
TMP="$HERE/dist/initramfs.cpio"
python3 "$HERE/mkcpio.py" "$INIT" "$TMP"
# Stream the archive instead of gzipping in place and renaming: the default
# OUT is $TMP.gz, so an unconditional mv would move the file onto itself.
gzip -n -9 -c "$TMP" > "$OUT"
rm -f "$TMP"
echo "wrote $OUT"
