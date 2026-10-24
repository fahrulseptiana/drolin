# kernel

One job: turn the pinned upstream kernel into a host-native User-Mode Linux
binary.

```sh
./fetch.sh     # download and verify linux-7.2.12
./build.sh     # defconfig + minimal fragment + build
```

`latest-stable.env` is the single source of truth for the version and digest.
The build is reproducible by default: fixed timestamp, fixed build user, and no
version string derived from the working tree.

## Architecture

Upstream mainline implements `ARCH=um` under `arch/x86/um`. The pinned tree
therefore boots on x86_64 out of the box. `patches/0001..0016` adds the arm64
subarch used by Android devices, one layer per patch: UML build plumbing,
generic headers, shared headers, SKAS kernel core, trap and arch init, host OS
adapter, host SKAS process control, then the arm64 subarch's kbuild and asm
headers, sysdep headers, bionic symbol redefinitions, CPU and fault handling,
ptrace, syscalls, signals and the vDSO. `build.sh` applies the series once, in
filename order, on an arm64 host. The patches are part of this repository so
the guest can be reproduced without a second source tree.

The Rust engine does not assume either architecture: it probes the ELF header
and validates it against the host at runtime.

The minimal config enables `CONFIG_UML_NET_VECTOR=y`. It is compatible with
`CONFIG_STATIC_LINK=y` because the series removes the over-broad
`select MAY_HAVE_RUNTIME_DEPS`; the vector `fd` transport does not resolve
hostnames.

The same fragment enables what a container runtime needs inside the guest: PID
and user namespaces, memory/pids cgroups, overlayfs, veth/bridge, and nftables
plus legacy iptables NAT.

## Android

`build-android.sh` produces the same guest for an app. It links statically
against the NDK sysroot and bionic; a glibc kernel dies under Android's zygote
seccomp filter before it can print anything.

```sh
NDK=/path/to/ndk ./build-android.sh
```

Outputs `dist/linux-um-aarch64-android` and `dist/stub_exe-aarch64-android`.
