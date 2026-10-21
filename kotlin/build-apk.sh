#!/bin/sh
# Build a signed debug APK with the Android SDK tools and the Kotlin compiler.
# No Gradle, no AndroidX: the app is one Activity, one Service and two widgets
# over the Rust engine.
set -eu

HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH= cd -- "$HERE/.." && pwd)
APP="$HERE/app/src/main"

SDK=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
[ -d "$SDK" ] || { echo "Android SDK not found; set ANDROID_HOME" >&2; exit 2; }

pick() {
    for candidate in "$@"; do
        [ -e "$candidate" ] && printf '%s\n' "$candidate"
    done
}
BT=${BT:-$(pick "$SDK"/build-tools/* | sort -V | tail -1)}
ANDROID_JAR=${ANDROID_JAR:-$(pick "$SDK"/platforms/android-35/android.jar "$SDK"/platforms/android-*/android.jar | sort -V | tail -1)}
[ -n "$BT" ] && [ -n "$ANDROID_JAR" ] || { echo "build-tools or android.jar missing" >&2; exit 2; }

KOTLIN_VERSION=${KOTLIN_VERSION:-2.4.21}
KOTLIN_HOME=${KOTLIN_HOME:-$HERE/.tools/kotlin-$KOTLIN_VERSION}
if [ ! -x "$KOTLIN_HOME/bin/kotlinc" ]; then
    mkdir -p "$HERE/.tools"
    ARCHIVE="$HERE/.tools/kotlin-$KOTLIN_VERSION.zip"
    if [ ! -f "$ARCHIVE" ]; then
        echo "fetching Kotlin $KOTLIN_VERSION"
        curl -fL --retry 3 --progress-bar -o "$ARCHIVE.part" \
            "https://github.com/JetBrains/kotlin/releases/download/v$KOTLIN_VERSION/kotlin-compiler-$KOTLIN_VERSION.zip"
        mv "$ARCHIVE.part" "$ARCHIVE"
    fi
    rm -rf "$HERE/.tools/kotlin-$KOTLIN_VERSION"
    unzip -q "$ARCHIVE" -d "$HERE/.tools"
    mv "$HERE/.tools/kotlinc" "$KOTLIN_HOME"
fi

ABI=${ABI:-arm64-v8a}
case "$ABI" in
    arm64-v8a)
        RUST_TARGET=aarch64-linux-android
        MUSL_TARGET=aarch64-unknown-linux-musl
        KERNEL_ARCH=aarch64
        ;;
    x86_64)
        RUST_TARGET=x86_64-linux-android
        MUSL_TARGET=x86_64-unknown-linux-musl
        KERNEL_ARCH=x86_64
        ;;
    *)
        echo "unsupported ABI: $ABI" >&2
        exit 2
        ;;
esac

NDK=${NDK:-$(pick "$SDK"/ndk/* | sort -V | tail -1)}
[ -n "$NDK" ] || { echo "Android NDK not found" >&2; exit 2; }
NDK_HOST=$(ls "$NDK/toolchains/llvm/prebuilt" | head -1)
LINKER="$NDK/toolchains/llvm/prebuilt/$NDK_HOST/bin/${RUST_TARGET}26-clang"
[ -x "$LINKER" ] || { echo "linker not found: $LINKER" >&2; exit 2; }

echo "building the Rust engine for $RUST_TARGET"
rustup target add "$RUST_TARGET" >/dev/null 2>&1 || true
UPPER=$(printf '%s' "$RUST_TARGET" | tr 'a-z-' 'A-Z_')
env "CARGO_TARGET_${UPPER}_LINKER=$LINKER" \
    cargo build --manifest-path "$ROOT/Cargo.toml" --release --target "$RUST_TARGET" -p drolin-cli

JNI="$APP/jni/$ABI"
mkdir -p "$JNI" "$APP/assets"
cp -f "$ROOT/target/$RUST_TARGET/release/drolin" "$JNI/libdrolin.so"
chmod 755 "$JNI/libdrolin.so"

INITRAMFS=${INITRAMFS:-$ROOT/images/dist/initramfs-$ABI.cpio.gz}
if [ ! -f "$INITRAMFS" ]; then
    TARGET="$MUSL_TARGET" OUT="$INITRAMFS" "$ROOT/images/mkinitramfs.sh"
fi
cp -f "$INITRAMFS" "$APP/assets/initramfs.cpio.gz"

KERNEL=${KERNEL:-$ROOT/kernel/dist/linux-um-$KERNEL_ARCH}
if [ -f "$KERNEL" ]; then
    cp -f "$KERNEL" "$JNI/liblinux.so"
    chmod 755 "$JNI/liblinux.so"
else
    rm -f "$JNI/liblinux.so"
    echo "WARNING: no kernel for $ABI at $KERNEL" >&2
    echo "WARNING: the APK will install, but the app will report a missing kernel" >&2
fi

BUILD="$HERE/build"
rm -rf "$BUILD"
mkdir -p "$BUILD/classes" "$BUILD/dex" "$BUILD/res"

echo "compiling Kotlin"
"$KOTLIN_HOME/bin/kotlinc" \
    -jvm-target 11 \
    -classpath "$ANDROID_JAR:$KOTLIN_HOME/lib/kotlin-stdlib.jar" \
    -d "$BUILD/classes" \
    "$APP"/java/dev/drolin/*.kt

jar cf "$BUILD/classes.jar" -C "$BUILD/classes" .
"$BT/d8" --release --min-api 26 --lib "$ANDROID_JAR" \
    --output "$BUILD/dex" "$BUILD/classes.jar" "$KOTLIN_HOME/lib/kotlin-stdlib.jar"

echo "packaging resources"
"$BT/aapt2" compile --dir "$APP/res" -o "$BUILD/res.zip"
"$BT/aapt2" link \
    -o "$BUILD/base.apk" \
    -I "$ANDROID_JAR" \
    --manifest "$APP/AndroidManifest.xml" \
    -A "$APP/assets" \
    --min-sdk-version 26 \
    --target-sdk-version 35 \
    --version-code 1 \
    --version-name 0.1.0 \
    "$BUILD/res.zip"

(cd "$BUILD/dex" && zip -q ../base.apk classes.dex)
mkdir -p "$BUILD/lib/$ABI"
cp -f "$JNI"/* "$BUILD/lib/$ABI"/
(cd "$BUILD" && zip -q -r base.apk "lib/$ABI")

KEYSTORE=${KEYSTORE:-$HERE/debug.keystore}
if [ ! -f "$KEYSTORE" ]; then
    keytool -genkeypair -keystore "$KEYSTORE" -storepass android -keypass android \
        -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" \
        -keyalg RSA -keysize 2048 -validity 10000 >/dev/null 2>&1
fi
"$BT/zipalign" -f 4 "$BUILD/base.apk" "$BUILD/aligned.apk"
mkdir -p "$HERE/dist"
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android --key-pass pass:android \
    --out "$HERE/dist/drolin.apk" "$BUILD/aligned.apk"
"$BT/apksigner" verify --print-certs "$HERE/dist/drolin.apk" >/dev/null
echo "wrote $HERE/dist/drolin.apk"
