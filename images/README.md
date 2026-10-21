# images

Two guest payloads, both reproducible from upstream inputs.

```sh
./mkinitramfs.sh     # minimal drolin-init, no root filesystem
./build-ubuntu.sh    # Ubuntu 26.04.1 arm64 ext4, needs mke2fs and root/sudo
```

`build-ubuntu.sh` verifies `ubuntu-base-26.04.1-base-arm64.tar.gz` against
Ubuntu's `SHA256SUMS`, repairs symlinks that make `mke2fs -d` fail, adds an
`/init` that mounts the pseudo-filesystems and starts bash, and shrinks the
filesystem with `resize2fs -M`. It never loop-mounts anything.

`mkinitramfs.sh` cross-compiles `drolin-init` for a Linux musl target with
`rust-lld` and writes a newc archive directly in Python. It works on macOS and
in a minimal Linux container.

Outputs go to `images/dist/`, which is ignored.
