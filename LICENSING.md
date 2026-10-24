# Licensing

`LICENSE` in the repository root is the GNU General Public License version 2
only (GPL-2.0-only) and covers everything here unless a directory states
otherwise. The split exists because three kinds of code meet in one product:
one engine, patches against GPL trees, and an Android app built on Apache-2.0
terminal libraries.

| path | license | why |
| --- | --- | --- |
| `rust/` (`drolin-core`, `drolin-runtime`, `drolin-cli`, `drolin-init`) | GPL-2.0-only | the engine, the launcher and the guest init; see `license` in `Cargo.toml` |
| `kernel/`, `images/`, `net/`, `docs/`, root `Makefile` | GPL-2.0-only | build and image scripts and documentation |
| `kernel/patches/` | GPL-2.0-only | patches applied to the Linux kernel, which is GPL-2.0-only |
| `net/patches/` | GPL-2.0-or-later | patches applied to passt, which is GPL-2.0-or-later |
| `android/` (app sources, JNI glue, `VENDORED-TERMUX.md`) | Apache-2.0 | the app is a combined work with the vendored Termux terminal libraries; see `android/LICENSE` and `android/LICENSE-TERMUX` |

## Third-party code that is fetched, not redistributed

Nothing built or downloaded is committed, so these stay under their own terms
and are pulled in at build time:

| source | fetched by | license |
| --- | --- | --- |
| Linux kernel source | `kernel/fetch.sh` | GPL-2.0-only WITH Linux-syscall-note |
| passt | `net/build-passt.sh` | GPL-2.0-or-later (plus `BSD-3-Clause` parts) |
| Ubuntu 26.04.1 base rootfs | `images/build-ubuntu.sh` | per package, as shipped in the image |
| Kotlin compiler | `android/build-apk.sh` | Apache-2.0 |
| Android SDK, NDK, build-tools | `android/build-apk.sh`, CI | Apache-2.0 and the Android SDK terms |
| Rust crates from crates.io | `cargo` | per crate, recorded in `Cargo.lock` |

The vendored Termux terminal is the one exception: it is committed on purpose
(there is no Gradle build to fetch it) and keeps Apache-2.0, see
`android/VENDORED-TERMUX.md` for the exact upstream commit.

## What a distributed APK contains

The APK bundles the GPL-2.0-only kernel, the GPL-2.0-or-later passt binary and
the Ubuntu image next to the Apache-2.0 app. They are separate works that talk
over a process boundary and a socket, so the app stays Apache-2.0 while the
bundled binaries stay GPL. Corresponding source is the pinned upstream tree
named by each build script (`kernel/latest-stable.env`, the passt commit in
`net/build-passt.sh`) plus the patches in this repository.
