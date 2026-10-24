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
│ pinned mainline, arm64 patch series, │
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
reproduced from a log or a test. `BootPlan` is the executable and argv the
runtime will use; it is rendered in a fixed order so plans compare equal.

Validation is pure. `validate_on(Some(host))` adds host-dependent rules (guest
page size cannot be smaller than the host's). The runtime reads `HostFacts`
once and passes them in.

## Boot path

1. The CLI/app builds a `GuestSpec`.
2. `BootPlan::from_spec` renders the UML command line.
3. `drolin-runtime::launch` validates again against the real host, starts the
   kernel, and attaches a console:
   * default: stdout/stderr to a regular file, tailed into the launcher and the
     marker scanner. UML's fd console polls its descriptors, and guest writes
     stall when those are pipes on Android; a file does not;
   * `--pty`: a pseudoterminal bridged to the launcher's stdin/stdout.
4. A marker is the success criterion when requested. A guest that intentionally
   stays up is not a failure after it has printed the marker.

## Kernel project

`kernel/latest-stable.env` pins the source version and SHA-256, `fetch.sh`
verifies and extracts it, and `build.sh` builds for the host with `ARCH=um`,
applying the arm64 subarch series `patches/0001..0016` on arm64.

`build-android.sh` builds the same guest against bionic, which the app needs: a
glibc kernel is a valid `adb shell` binary but dies under zygote's seccomp
filter, and Android has no glibc loader.

## Networking

Every privileged UML transport is unavailable to an unprivileged app: `tap`
needs `/dev/net/tun` and `CAP_NET_ADMIN`, `raw` needs `CAP_NET_RAW`, and the
others need a switch daemon. The vector driver's `fd` transport needs only a
descriptor somebody else created, so the guest's network is a userspace stack
fed by a bridge in `drolin-runtime/src/net.rs`:

```
UML  <--SOCK_DGRAM, one frame per datagram-->  bridge
bridge  <--SOCK_STREAM, 4-byte length prefix-->  passt
passt  <--ordinary TCP/UDP/ICMP sockets-->  host network
```

passt is passed an explicit address, route and nameserver instead of
discovering them: netlink is denied on Android, the routing table is per-uid,
and there is no `/etc/resolv.conf`. It only needs outbound sockets, which the
`INTERNET` permission allows. See `net/README.md` for the flags, the bionic
build and port forwarding.

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
with `--inherit`, so UML's `con0=fd:0,fd:1` is that pty.

`TMPDIR` is pointed at the app cache, the writable location where `PROT_EXEC`
mmap is permitted. The app boots on launch, and a finished session (shell `exit`
or `poweroff`) stops the foreground service, which keeps the guest alive while
the Activity is not visible.

## Ubuntu image

`images/build-ubuntu.sh` verifies the official base tarball, installs systemd
through a chroot, and populates an ext4 image with `mke2fs -d` without mounting
anything. The image is compressed into the APK and expanded once into app data.

systemd is PID 1 (`/sbin/init`) with a `getty@tty0` autologin.
`/usr/local/bin/drolin-shell` runs bash and powers off when bash exits, so
`exit` ends the UML process. `/usr/local/bin/drolin-marker` prints
`DROLIN_GUEST_OK` only with the `drolin.poweroff=1` kernel argument, so
interactive boots keep the prompt clean. `/usr/sbin/drolin-poweroff` is exposed
as `poweroff` and `halt`.

## What is deliberately absent

* QEMU, KVM or root on the host: a guest is an ordinary process.
* Build output in git: `dist/`, `build/` and `target/` are ignored.
