# drolin

A small Linux guest runner for arm64, built from three independent projects
that share one engine:

| project | language | responsibility |
| --- | --- | --- |
| `rust/` | Rust | planning, validation, host probes, image storage, process lifecycle, guest init |
| `kernel/` | shell + Kconfig | pinned stable mainline source, arm64 UML patch, host and Android builds |
| `kotlin/` | Kotlin | Android app: UI, foreground service, packaged engine and Ubuntu image |

The engine is the only place that knows what a boot is. The CLI and the Android
app are adapters; neither re-implements kernel arguments or validation.

## Quick start

On an arm64 Linux host (or a Linux arm64 container):

```sh
cargo test
make kernel                 # latest stable mainline + arm64 UML subarch
make guest                  # build the demo initramfs
cargo run -p drolin-cli -- run \
  --kernel kernel/dist/linux-um-aarch64 \
  --initramfs images/dist/initramfs.cpio.gz \
  --marker DROLIN_GUEST_OK --timeout 60
```

Ubuntu 26.04.1 arm64 as the guest:

```sh
make kernel
./images/build-ubuntu.sh    # needs root or sudo for mke2fs -d
cargo run -p drolin-cli -- run \
  --kernel kernel/dist/linux-um-aarch64 \
  --rootfs images/dist/ubuntu-26.04.1-arm64.ext4 \
  --init /init --marker DROLIN_GUEST_OK
```

Inside the guest you get a root bash shell. `uname -a` reports the pinned
kernel version (`7.2.12-drolin`).

## Android

```sh
# Linux host with Android SDK + NDK:
NDK=/path/to/ndk ./kernel/build-android.sh
ANDROID_HOME=/path/to/sdk ./kotlin/build-apk.sh
adb install -r kotlin/dist/drolin.apk
```

The APK bundles the bionic kernel, the standalone stub, a demo initramfs and
the compressed Ubuntu image. The app boots it automatically on launch, expands
the image once into app data, and hands the console to a vendored Termux
terminal. `exit` or `poweroff` in the guest ends the session and stops the
foreground service. The APK is signed with the debug key and is not a release
artifact.

## Layout

```
rust/drolin-core      domain model, no I/O
rust/drolin-runtime   host adapters and launcher
rust/drolin-cli       the `drolin` binary
rust/drolin-init      guest /init and drolin-poweroff
kernel/               pinned source, verification, UML builds
images/               root filesystem and initramfs builders
kotlin/               Android app and SDK-tool build script
docs/                 architecture and rationale
```

Nothing built is committed: kernel trees, images, APKs, native libraries and
cargo output are all ignored. See `docs/ARCHITECTURE.md` for the design and the
platform constraints that shaped it.
