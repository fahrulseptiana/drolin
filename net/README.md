# passt network backend

The guest's network is a userspace TCP/IP stack, not a tap device.

```
guest vec0  <--frames-->  UML vector fd  <--datagrams-->  drolin runtime
                                                          <--length stream-->
                                                          passt
                                                          <--ordinary sockets-->
                                                          host network
```

UML's `fd` transport carries one Ethernet frame per datagram. passt speaks
qemu's stream protocol, with each frame prefixed by a 4-byte big-endian length.
The Rust runtime owns the socketpairs and the reframing pump; no `umnet`
process is needed.

## Build

```sh
./build-passt.sh                 # static host binary, net/dist/passt-host
MODE=android NDK=... ./build-passt.sh
```

The Android build applies `patches/passt-bionic.patch` and links statically
against the NDK sysroot. It is pinned to a known passt commit. The APK packages
it as `lib/arm64-v8a/libpasst.so`.

## Why the configuration is explicit

passt normally discovers the host's addresses, routes and nameservers through
netlink and `/etc/resolv.conf`. An unprivileged Android app cannot read
netlink, has a per-uid routing table, and has no resolv.conf. The runtime passes
the whole configuration instead:

```text
-4 -a 10.0.2.15 -n 24 -g 10.0.2.2 -D 1.1.1.1
```

passt still reaches the network through ordinary outbound sockets, which is why
this works without root.

## Port forwarding

`passt -t host:guest` / `-u host:guest` exposes guest TCP/UDP ports on the
Android host. The engine accepts repeated `--forward tcp:8080:80` flags; the
app persists the same list and applies it on the next boot. Host ports below
1024 need root and are rejected by the app.

## Guest side

The Ubuntu image runs systemd-networkd with DHCP on `vec0`. passt is the DHCP
server; the guest gets `10.0.2.15/24`, a default route through `10.0.2.2`, and
DNS `1.1.1.1`.
