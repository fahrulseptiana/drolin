#!/usr/bin/env python3
"""Write a reproducible newc cpio archive without external tools.

The guest does not need a distribution to prove that the kernel booted: one
static /init, four empty directories and a marker are enough. Keeping the
writer here makes the image buildable on macOS and in a minimal CI image.
"""
import os
import stat
import sys

MAGIC = b"070701"


def entry(name: str, mode: int, data: bytes = b"", ino: int = 0) -> bytes:
    name_bytes = name.encode() + b"\0"
    fields = [
        ino,
        mode,
        0,  # uid
        0,  # gid
        1,  # nlink
        0,  # mtime
        len(data),
        0,  # devmajor
        0,  # devminor
        0,  # rdevmajor
        0,  # rdevminor
        len(name_bytes),
        0,  # check
    ]
    header = MAGIC + b"".join(f"{value:08x}".encode() for value in fields)
    assert len(header) == 110
    out = bytearray(header)
    out += name_bytes
    out += b"\0" * ((4 - len(out) % 4) % 4)
    out += data
    out += b"\0" * ((4 - len(data) % 4) % 4)
    return bytes(out)


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: mkcpio.py INIT OUTPUT", file=sys.stderr)
        return 2
    init_path, output = sys.argv[1], sys.argv[2]
    with open(init_path, "rb") as handle:
        init = handle.read()
    if not init.startswith(b"\x7fELF"):
        print(f"{init_path} is not an ELF binary", file=sys.stderr)
        return 1

    ino = 1
    archive = bytearray()

    def add(name: str, mode: int, data: bytes = b"") -> None:
        nonlocal ino
        archive.extend(entry(name, mode, data, ino))
        ino += 1

    add(".", stat.S_IFDIR | 0o755)
    for directory in ("dev", "proc", "sys", "tmp", "run"):
        add(directory, stat.S_IFDIR | 0o755)
    add("init", stat.S_IFREG | 0o755, init)
    add("TRAILER!!!", 0)

    with open(output, "wb") as handle:
        handle.write(archive)
    print(f"{output}: {len(archive)} bytes, init={init_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
