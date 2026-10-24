# drolin

A small Linux guest runner for arm64, built from three independent projects
that share one engine:

| project | language | responsibility |
| --- | --- | --- |
| `rust/` | Rust | planning, validation, host probes, image storage, process lifecycle, guest init |
| `kernel/` | shell + Kconfig | pinned stable mainline source, arm64 UML patch series, host and Android builds |
| `android/` | Kotlin | Android app: UI, foreground service, packaged engine and Ubuntu image |

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
  --init /sbin/init --marker DROLIN_GUEST_OK
```

systemd is PID 1. You get a root bash shell through an autologin on the UML
console. `uname -a` reports the pinned kernel version (`7.2.12`), and `systemctl
status` is available.

## Networking

The guest gets DHCP, DNS, ICMP and TCP without root, a tap device or user
namespaces. UML's vector `fd` transport is bridged by the Rust runtime to
passt, a userspace TCP/IP stack that reaches the network through ordinary
sockets.

```sh
make passt                         # host static passt
cargo run -p drolin-cli -- run \
  --kernel kernel/dist/linux-um-aarch64 \
  --rootfs images/dist/ubuntu-26.04.1-arm64.ext4 \
  --init /sbin/init --net --passt net/dist/passt-host
```

The guest interface is `vec0`; systemd-networkd obtains `10.0.2.15/24` from
passt and uses `1.1.1.1` for DNS. Host-to-guest ports can be forwarded with
repeated `--forward tcp:8080:80` options. See `net/README.md`.

## Android

```sh
# Linux host with Android SDK + NDK:
NDK=/path/to/ndk ./kernel/build-android.sh
ANDROID_HOME=/path/to/sdk ./android/build-apk.sh
adb install -r android/dist/drolin.apk
```

The APK bundles the bionic kernel, the standalone stub, a demo initramfs and
the compressed Ubuntu image. The app boots it automatically on launch, expands
the image once into app data, and hands the console to a vendored Termux
terminal. `exit` or `poweroff` in the guest ends the session and stops the
foreground service. The APK is debug-signed, and tagged releases publish it as
`drolin-<tag>-arm64-v8a-debug.apk`.

## Layout

```
rust/drolin-core      domain model, no I/O
rust/drolin-runtime   host adapters and launcher
rust/drolin-cli       the `drolin` binary
rust/drolin-init      guest /init and drolin-poweroff
kernel/               pinned source, verification, UML builds
images/               root filesystem and initramfs builders
android/              Android app and SDK-tool build script
docs/                 architecture and rationale
```

Nothing built is committed: kernel trees, images, APKs, native libraries and
cargo output are all ignored. See `docs/ARCHITECTURE.md` for the design and the
platform constraints that shaped it.

## License

The engine, the build scripts and the patches are GPL-2.0-only; the Android app
is Apache-2.0 because it is a combined work with the vendored Termux terminal.
`LICENSING.md` maps every directory, and the sources fetched at build time
(kernel, passt, Ubuntu base, Kotlin, Android SDK) keep their own licenses.
