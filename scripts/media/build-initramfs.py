#!/usr/bin/env python3
"""Package reviewed static init/worker binaries into a deterministic newc image."""
import argparse
import pathlib
import stat


def entry(name, mode, data=b"", ino=1):
    encoded = name.encode("ascii") + b"\0"
    fields = [ino, mode, 0, 0, 1, 0, len(data), 0, 0, 0, 0, len(encoded), 0]
    header = b"070701" + b"".join(f"{field:08x}".encode() for field in fields)
    prefix = header + encoded
    return prefix + bytes(-len(prefix) % 4) + data + bytes(-len(data) % 4)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("worker", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    args = parser.parse_args()
    binary = args.binary.read_bytes()
    worker = args.worker.read_bytes()
    if any(len(data) > 16 * 1024 * 1024 or not data.startswith(b"\x7fELF") for data in (binary, worker)):
        parser.error("expected a bounded ELF guest binary")
    image = entry("init", stat.S_IFREG | 0o755, binary)
    image += entry("worker", stat.S_IFREG | 0o755, worker, ino=2)
    for ino, name in enumerate(("dev", "proc", "sys"), 3):
        image += entry(name, stat.S_IFDIR | 0o755, ino=ino)
    image += entry("TRAILER!!!", 0, ino=6)
    with args.output.open("xb") as output:
        output.write(image)


if __name__ == "__main__":
    main()
