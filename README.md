# drolin

A small, auditable Linux guest runner built from three independent projects:

| project | language | responsibility |
| --- | --- | --- |
| `rust/` | Rust | the engine: planning, validation, probing, image storage, process lifecycle |
| `kernel/` | shell + Kconfig | fetch and build a pinned mainline Linux kernel for User-Mode Linux |
| `kotlin/` | Kotlin | Android front end: terminal, notifications, packaging |

The engine is the only place that knows what a boot is. The CLI and the Android
app are adapters over it; neither re-implements kernel arguments or validation.

## Quick start

```sh
cargo test
cargo run -p drolin-cli -- plan --kernel /path/to/linux --initramfs /path/to/initrd
cargo run -p drolin-cli -- doctor --kernel /path/to/linux
```

Build a guest kernel and a five-second demonstration initramfs on Linux:

```sh
make kernel          # fetches the pinned stable source and builds ARCH=um
make guest           # builds drolin-init and packs it into an initramfs
./target/release/drolin run \
  --kernel kernel/dist/linux-um \
  --initramfs images/dist/initramfs.cpio.gz \
  --marker DROLIN_GUEST_OK
```

The design and the exact compatibility rules are in `docs/ARCHITECTURE.md`.
