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
therefore boots on x86_64 out of the box. `patches/um-arm64.patch` adds the
arm64 subarch used by Android devices; `build.sh` applies it once on an arm64
host. The patch is part of this repository so the guest can be reproduced
without a second source tree.

The Rust engine does not assume either architecture: it probes the ELF header
and validates it against the host at runtime.
