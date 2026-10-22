# Architecture

drolin is three independent projects with one source of truth for boot policy.

```
┌──────────────────────────────┐
│ rust/drolin-core             │  pure domain: specs, validation,
│                              │  kernel arguments, markers, state
└──────────────┬───────────────┘
               │
┌──────────────▼───────────────┐
│ rust/drolin-runtime          │  host adapters: ELF probe, image store,
│                              │  process launch, pty, logging
└──────────────┬───────────────┘
               │
       ┌───────┴────────┐
       │                │
┌──────▼──────┐  ┌──────▼──────────────┐
│ drolin CLI  │  │ Kotlin Android app  │
│ (same bin)  │  │ (UI + lifecycle)    │
└─────────────┘  └─────────────────────┘
       │
┌──────▼───────────────────────────────┐
│ kernel/ + images/                    │
│ pinned mainline source, arm64 patch, │
│ Ubuntu root filesystem, initramfs    │
└──────────────────────────────────────┘
```

## The rule

The **engine** is the only project that knows what a boot is. The CLI, the
Android app, the kernel build and the image build are adapters around it. The
Android app does not construct a kernel command line and the CLI does not
duplicate validation.

## Rust crates

| crate | responsibility | allowed to do I/O? |
| --- | --- | --- |
| `drolin-core` | `GuestSpec`, validation, `BootPlan`, marker scanner, image manifest, session state machine | no |
| `drolin-runtime` | ELF probe, host facts, image store, launch, pty bridge, log | yes |
| `drolin-cli` | argument parsing, rendering, exit codes | yes |
| `drolin-init` | guest `/init` and `drolin-poweroff` | Linux guest only |

`GuestSpec` is the contract. It is serialisable, so the same request can be
reproduced from a log, a test, or a future UI. `BootPlan` is the executable and
argv the runtime will use; it is rendered in a fixed order so plans compare
equal.

Validation is pure. `validate_on(Some(host))` adds host-dependent rules (guest
page size cannot be smaller than the host's). The runtime reads `HostFacts`
once and passes them in.

## Boot path

1. The CLI/app builds a `GuestSpec`.
2. `BootPlan::from_spec` renders the UML command line.
3. `drolin-runtime::launch` validates again against the real host, starts the
   kernel, and attaches a console:
   * default: stdout/stderr to a regular file, tailed into the launcher and the
     marker scanner;
   * `--pty`: a pseudoterminal bridged to the launcher's stdin/stdout.
4. A marker is the success criterion when requested. A guest that intentionally
   stays up is not a failure after it has printed the marker.

The file-backed console exists because UML's fd console uses poll/epoll on its
descriptors, and on Android guest writes stall when those descriptors are pipes.
A file works. The pty path is the interactive alternative.

## Why a file and not a pipe on Android

Observed on a physical arm64 device: kernel messages reached the launcher, but
the guest's first `write(1)` never appeared. The same kernel and root filesystem
worked in a Linux container. Attaching the kernel's stdout/stderr to a regular
file and tailing it fixed the guest without changing the kernel. This is a host
console detail, not a UML policy, so it lives in `drolin-runtime`.

## Kernel project

`kernel/latest-stable.env` pins the source version and SHA-256. `fetch.sh`
verifies the tarball and extracts it. `build.sh` builds for the host with
`ARCH=um`; on arm64 it applies `patches/um-arm64.patch` once. `build-android.sh`
compiles statically against the NDK sysroot and bionic for an app.

Bionic is required for the app. A glibc kernel is a valid `adb shell` binary but
dies under zygote's seccomp filter, and Android has no glibc loader.

## Networking

Every privileged UML transport is unavailable to an unprivileged app: `tap`
needs `/dev/net/tun` and `CAP_NET_ADMIN`, `raw` needs `CAP_NET_RAW`, and the
others need a switch daemon. The vector driver's `fd` transport needs only a
descriptor somebody else created.

The runtime creates two socketpairs:

```
UML  <--SOCK_DGRAM, one frame per datagram-->  bridge
bridge  <--SOCK_STREAM, 4-byte length prefix-->  passt
passt  <--ordinary TCP/UDP/ICMP sockets-->  host network
```

The bridge is Rust (`drolin-runtime/src/net.rs`), so the old C `umnet` helper is
not needed. passt starts with `-F <fd> -t none -u none` and an explicit network:

```text
-4 -a 10.0.2.15 -n 24 -g 10.0.2.2 -D 1.1.1.1
```

Explicit configuration is load-bearing on Android: netlink is denied, the
routing table is per-uid, and there is no `/etc/resolv.conf`. passt only needs
outbound sockets, which an app may open with the `INTERNET` permission.

The guest image enables systemd-networkd with DHCP on `vec0`. It receives
`10.0.2.15/24`, a route through `10.0.2.2`, and DNS `1.1.1.1`.

Host-to-guest forwarding is passed to passt as `-t host:guest` (TCP) and
`-u host:guest` (UDP). The app persists the list and passes repeated
`--forward tcp:8080:80` options; a restart applies them.

`net/build-passt.sh` builds a static host binary and, with `MODE=android`, a
bionic binary for the APK. The Android build applies the pinned bionic patch.

## Guest page size

A guest page is a host `mmap()`, so:

```
guest page size >= host page size
```

The default is 16 KiB, which runs on 4 KiB and 16 KiB hosts. A 4 KiB guest does
not boot on a 16 KiB host; it hangs, which is why the planner rejects it before
launch when the size is known.

## Android packaging

Android refuses to execute files an app wrote, and refuses `execveat` on a
memfd. The engine, kernel and stub are therefore shipped as `lib*.so` files in
`lib/arm64-v8a/`, the one directory Android unpacks and allows executing. They
are ordinary executables with a library-shaped name.

The console is a real pty. The app vendors Termux's `terminal-emulator` and
`terminal-view` (Apache-2.0; see `android/VENDORED-TERMUX.md`) and runs the Rust
engine through `TerminalSession`, which creates the pty. The engine is started
with `--inherit`, so UML's `con0=fd:0,fd:1` is that pty. The view is focusable
in touch mode and has the terminal's black background set explicitly; the
renderer paints only non-default cell backgrounds.

`TMPDIR` is pointed at the app cache, the writable location where `PROT_EXEC`
mmap is permitted. The app boots on launch, and a finished terminal session
(shell `exit` or `poweroff`) stops the foreground service. The service keeps the
guest alive when the Activity is not visible.

## Ubuntu image

`images/build-ubuntu.sh` verifies the official base tarball, installs systemd
into a chroot, configures a `getty@tty0` autologin, fixes absolute and dangling
symlinks, and populates an ext4 image with `mke2fs -d` without mounting it. The
image is compressed into the APK and expanded once into app data.

systemd is PID 1 (`/sbin/init`). It starts dbus, udev, logind, journald and the
console getty. `/usr/local/bin/drolin-shell` runs bash and powers off when bash
exits, so `exit` ends the UML process. `/usr/local/bin/drolin-marker` prints
`DROLIN_GUEST_OK` only with the `drolin.poweroff=1` kernel argument, then powers
off for automated checks. Interactive boots print no marker, keeping the shell
prompt clean. `/usr/sbin/drolin-poweroff` is exposed as `poweroff` and
`halt`.

The base tarball's systemd postinst calls `systemd-machine-id-setup`, which
reopens `/etc/machine-id` through `/proc/self/fd`. A chroot build has no
`/proc`, so the script seeds a machine-id and neutralises that one helper
before `dpkg --configure -a`. The device nodes needed by apt are empty regular
files; devtmpfs replaces them at boot, and they keep systemd's `/dev/null`
generator masks resolvable for mke2fs.

## What is deliberately absent

* No Docker, QEMU, KVM, root, or package manager at runtime.
* No network yet.
* No binary, kernel image, root filesystem or APK in git. Build artifacts live
  under ignored `dist/`, `build/` and `target/` directories.
