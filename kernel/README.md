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

Upstream mainline implements `ARCH=um` under `arch/x86/um`. It does not ship an
`arch/arm64/um` subarch. `build.sh` therefore builds on x86_64 Linux and stops
on arm64 with an explicit message rather than pretending. The Rust engine does
not assume either architecture; it probes the ELF header and validates it
against the host at runtime.
