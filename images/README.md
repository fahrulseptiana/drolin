# images

Two guest payloads, both reproducible from upstream inputs.

```sh
./mkinitramfs.sh     # minimal drolin-init, no root filesystem
./build-ubuntu.sh    # Ubuntu 26.04.1 arm64 ext4, needs mke2fs and root/sudo
```

`build-ubuntu.sh` verifies `ubuntu-base-26.04.1-base-arm64.tar.gz` against
Ubuntu's `SHA256SUMS`, installs systemd into a chroot, configures a
`getty@tty0` autologin, and shrinks the filesystem with `resize2fs -M`. It
never loop-mounts anything. The chroot device stand-ins are replaced by
devtmpfs at boot.

`mkinitramfs.sh` cross-compiles `drolin-init` for a Linux musl target with
`rust-lld` and writes a newc archive directly in Python. It works on macOS and
in a minimal Linux container.

`build-ubuntu.sh` needs an arm64 Linux host or container and root/sudo because
systemd is installed through a chroot. Cross-architecture APK builds consume
the ext4 artifact produced by an arm64 job.

Outputs go to `images/dist/`, which is ignored.
