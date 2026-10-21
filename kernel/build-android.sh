#!/bin/bash
# Build UML/arm64 against bionic for an Android app.
#
# The kernel is a userspace process, so its C library is part of the target:
# Android has no glibc loader, and zygote's seccomp filter rejects glibc's
# startup. This build uses the NDK sysroot and a static bionic link.
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
# shellcheck disable=SC1091
. "$HERE/latest-stable.env"

if [ "$(uname -s)" != "Linux" ]; then
    echo "the kernel must be built on Linux; use a container or CI" >&2
    exit 2
fi

NDK=${NDK:-${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}}
if [ -z "$NDK" ]; then
    for candidate in "$HOME"/Library/Android/sdk/ndk/* "$HOME"/Android/Sdk/ndk/* /opt/android-ndk*; do
        [ -d "$candidate" ] && NDK=$candidate
    done
fi
[ -n "$NDK" ] && [ -d "$NDK" ] || { echo "Android NDK not found; set NDK=" >&2; exit 2; }

HOSTPRE=
for candidate in linux-x86_64 linux-aarch64 darwin-x86_64; do
    if [ -d "$NDK/toolchains/llvm/prebuilt/$candidate/sysroot" ]; then
        HOSTPRE=$candidate
        break
    fi
done
[ -n "$HOSTPRE" ] || { echo "no NDK sysroot under $NDK" >&2; exit 2; }
SYSROOT="$NDK/toolchains/llvm/prebuilt/$HOSTPRE/sysroot"
API=${API:-30}
TARGET_CC=$(command -v clang)
[ -n "$TARGET_CC" ] || { echo "host clang is required" >&2; exit 2; }

"$HERE/fetch.sh"
TREE="$HERE/src/linux-$KERNEL_VERSION"
if [ ! -d "$TREE/arch/arm64/um" ]; then
    echo "applying the arm64 UML subarch patch"
    (cd "$TREE" && patch -p1 -f < "$HERE/patches/um-arm64.patch")
    chmod +x "$TREE/arch/arm64/um/vdso/gen_vdso_offsets.sh"
    touch "$TREE/arch/arm64/um/vdso/gen_vdso_offsets.sh"
fi

OUT=${DROLIN_KERNEL_OUT:-$HERE/build-android}
DIST=${DROLIN_KERNEL_DIST:-$HERE/dist}
JOBS=${JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)}
SHIM="$OUT/.ndk-shim"
mkdir -p "$SHIM/lib" "$OUT" "$DIST"
# Debian's clang still asks for -lgcc when linking this target. The NDK's
# compiler-rt archive is the equivalent; expose it under the name the driver
# looks for rather than pulling a GNU cross toolchain into the build.
BUILTINS=$(find "$NDK/toolchains/llvm/prebuilt/$HOSTPRE/lib/clang" \
    -name 'libclang_rt.builtins-aarch64-android.a' | head -1)
[ -n "$BUILTINS" ] && ln -sf "$BUILTINS" "$SHIM/lib/libgcc.a"
UNWIND=$(find "$NDK/toolchains/llvm/prebuilt/$HOSTPRE/lib/clang" \
    -path '*aarch64/libunwind.a' | head -1)
UNWIND_DIR=
[ -n "$UNWIND" ] && UNWIND_DIR=$(dirname "$UNWIND")
cat > "$SHIM/clang" <<WRAPPER
#!/bin/sh
exec "$TARGET_CC" "\$@" --target=aarch64-linux-android$API --sysroot="$SYSROOT" -L"$SHIM/lib" -L"$UNWIND_DIR" -lunwind -Wno-unused-command-line-argument
WRAPPER
cp "$SHIM/clang" "$SHIM/clang++"
chmod +x "$SHIM/clang" "$SHIM/clang++"
for tool in ld.lld llvm-ar llvm-nm llvm-objcopy llvm-objdump llvm-readelf llvm-strip; do
    command -v "$tool" >/dev/null 2>&1 && ln -sf "$(command -v "$tool")" "$SHIM/$tool"
done
export PATH="$SHIM:$PATH"
HOST_ARGS=(HOSTCC="$TARGET_CC" HOSTCXX="$TARGET_CC")
MAKE="make -C $TREE O=$OUT ARCH=um SUBARCH=arm64 LLVM=1"

export KBUILD_BUILD_TIMESTAMP=${KBUILD_BUILD_TIMESTAMP:-"Thu Jan  1 00:00:00 UTC 1970"}
export KBUILD_BUILD_USER=${KBUILD_BUILD_USER:-drolin}
export KBUILD_BUILD_HOST=${KBUILD_BUILD_HOST:-drolin}

# shellcheck disable=SC2086
$MAKE "${HOST_ARGS[@]}" defconfig
"$TREE/scripts/config" --file "$OUT/.config" \
    -e STATIC_LINK -d LOCALVERSION_AUTO --set-str LOCALVERSION "-drolin" \
    -e PAGE_SIZE_16KB -d PAGE_SIZE_4KB
"$TREE/scripts/kconfig/merge_config.sh" -m -O "$OUT" "$OUT/.config" "$HERE/config/minimal.config"
# shellcheck disable=SC2086
$MAKE "${HOST_ARGS[@]}" olddefconfig
grep -q '^CONFIG_PAGE_SIZE_16KB=y' "$OUT/.config" || {
    echo "16 KB guest pages were not selected" >&2
    exit 3
}
# shellcheck disable=SC2086
$MAKE "${HOST_ARGS[@]}" -j"$JOBS"

cp -f "$OUT/linux" "$DIST/linux-um-aarch64"
cp -f "$OUT/arch/um/kernel/skas/stub_exe" "$DIST/stub_exe-aarch64"
echo "wrote $DIST/linux-um-aarch64 (bionic, API $API)"
file "$DIST/linux-um-aarch64" || true
