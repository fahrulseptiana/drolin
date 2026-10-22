#!/bin/bash
# Build passt, the userspace TCP/IP stack behind the guest's vector interface.
#
#   ./net/build-passt.sh              # host static build
#   MODE=android ./net/build-passt.sh # bionic build for the APK
#
# The Android build uses the NDK's clang directly and applies the bionic header
# and isolation patch kept in net/patches. The result is static because an app
# may only exec from nativeLibraryDir and Android provides no glibc loader.
set -euo pipefail

HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/.." && pwd)
COMMIT=${PASST_COMMIT:-defc25b9444c508d21badb6bc9a0b835ddd01126}
MODE=${MODE:-host}
SRC=${PASST_SRC:-$HERE/src/passt}
OUT=${PASST_OUT:-$HERE/dist}
API=${API:-30}
PAGE=${PAGE:-16384}
BUILD="$HERE/build/$MODE"

mkdir -p "$(dirname "$SRC")" "$OUT"

if [ ! -d "$SRC/.git" ]; then
    echo "fetching passt"
    git clone --quiet https://passt.top/passt "$SRC"
fi
git -C "$SRC" checkout --quiet "$COMMIT"

rm -rf "$BUILD"
mkdir -p "$BUILD"
tar -C "$SRC" --exclude=.git -cf - . | tar -C "$BUILD" -xf -
cd "$BUILD"
rm -f passt pasta passt-repair pesto seccomp.h

case "$MODE" in
    host)
        make -s CC=gcc LDFLAGS=-static passt
        cp -f passt "$OUT/passt-host"
        echo "wrote $OUT/passt-host"
        file "$OUT/passt-host"
        ;;
    android)
        NDK=${NDK:-${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}}
        if [ -z "$NDK" ]; then
            for c in "$HOME"/Library/Android/sdk/ndk/* "$HOME"/Android/Sdk/ndk/* /opt/android-ndk*; do
                [ -d "$c" ] && NDK=$c
            done
        fi
        [ -n "$NDK" ] && [ -d "$NDK" ] || { echo "Android NDK not found (set NDK)" >&2; exit 2; }
        HOSTPRE=
        for c in linux-x86_64 linux-aarch64 darwin-x86_64; do
            [ -d "$NDK/toolchains/llvm/prebuilt/$c/sysroot" ] && { HOSTPRE=$c; break; }
        done
        [ -n "$HOSTPRE" ] || { echo "no NDK sysroot under $NDK" >&2; exit 2; }
        SYSROOT="$NDK/toolchains/llvm/prebuilt/$HOSTPRE/sysroot"
        TARGET_CC=$(command -v clang)
        [ -n "$TARGET_CC" ] || { echo "host clang is required" >&2; exit 2; }
        SHIM="$BUILD/.shim"
        mkdir -p "$SHIM/lib"
        BUILTINS=$(find "$NDK/toolchains/llvm/prebuilt/$HOSTPRE/lib/clang" \
            -name 'libclang_rt.builtins-aarch64-android.a' | head -1)
        [ -n "$BUILTINS" ] && ln -sf "$BUILTINS" "$SHIM/lib/libgcc.a"
        UNWIND=$(find "$NDK/toolchains/llvm/prebuilt/$HOSTPRE/lib/clang" \
            -path '*aarch64/libunwind.a' | head -1)
        UNWIND_DIR=
        [ -n "$UNWIND" ] && UNWIND_DIR=$(dirname "$UNWIND")
        CC="$TARGET_CC --target=aarch64-linux-android$API --sysroot=$SYSROOT -L$SHIM/lib -L$UNWIND_DIR -lunwind -Wno-unused-command-line-argument"
        patch -p1 --silent < "$HERE/patches/passt-bionic.patch"
        make -s CC="$CC" ARCH=aarch64 TARGET=aarch64-linux-android \
            VERSION="$(git -C "$SRC" describe --tags --always)-drolin" \
            CFLAGS="-static -Wl,-z,max-page-size=$PAGE" \
            CPPFLAGS="-UPAGE_SIZE -DPAGE_SIZE=$PAGE" \
            passt
        cp -f passt "$OUT/passt-android"
        readelf -h "$OUT/passt-android" | grep -q 'Machine:.*AArch64' || {
            echo "passt-android is not aarch64" >&2; exit 1;
        }
        if readelf -d "$OUT/passt-android" 2>/dev/null | grep -q NEEDED; then
            echo "passt-android is dynamically linked" >&2; exit 1;
        fi
        echo "wrote $OUT/passt-android"
        file "$OUT/passt-android"
        ;;
    *)
        echo "MODE must be host or android" >&2
        exit 2
        ;;
esac
