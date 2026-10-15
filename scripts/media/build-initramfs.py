#!/usr/bin/env python3
"""Package reviewed static init/worker binaries into a deterministic newc image."""
import argparse
import gzip
import json
import pathlib
import stat
from pdf_runtime import collect as collect_pdf_runtime

MAX_IMAGE = 64 * 1024 * 1024


def entry(name, mode, data=b"", ino=1):
    encoded = name.encode("ascii") + b"\0"
    fields = [ino, mode, 0, 0, 1, 0, len(data), 0, 0, 0, 0, len(encoded), 0]
    header = b"070701" + b"".join(f"{field:08x}".encode() for field in fields)
    prefix = header + encoded
    return prefix + bytes(-len(prefix) % 4) + data + bytes(-len(data) % 4)


def file_entries(files, ino):
    """Store identical read-only resource bytes once, with guest-local aliases."""
    resources = {}
    image = bytearray()
    for name, (mode, data) in sorted(files.items()):
        if mode == 0o644 and data in resources:
            image += entry(name, stat.S_IFLNK | 0o777,
                           ('/' + resources[data]).encode('ascii'), ino=ino)
        else:
            image += entry(name, stat.S_IFREG | mode, data, ino=ino)
            if mode == 0o644:
                resources[data] = name
        ino += 1
    return image, ino


def boot_image(image, *, pdf_runtime):
    if len(image) > MAX_IMAGE:
        raise ValueError(f'initramfs exceeds the existing artifact size budget: {len(image)} bytes')
    # The pinned qualification kernel enables CONFIG_RD_GZIP. Compress only
    # the PDF image so its kernel + initrd + disks fit the same private tmpfs.
    output = gzip.compress(bytes(image), mtime=0) if pdf_runtime else image
    if len(output) > MAX_IMAGE:
        raise ValueError('compressed initramfs exceeds the artifact size budget')
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("worker", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--pdf-runtime", action="store_true",
                        help="include the fixed installed Ghostscript runtime")
    parser.add_argument("--video-runtime", action="store_true",
                        help="include the fixed minimal video runtime")
    parser.add_argument("--still-runtime", action="store_true",
                        help="include the fixed lossless JPEG metadata cleaner")
    parser.add_argument("--gif-runtime", action="store_true",
                        help="include the fixed GIF metadata cleaner")
    args = parser.parse_args()
    if sum((args.pdf_runtime, args.video_runtime, args.still_runtime, args.gif_runtime)) > 1:
        parser.error("one explicit media runtime per image")
    binary = args.binary.read_bytes()
    worker = args.worker.read_bytes()
    if any(len(data) > 16 * 1024 * 1024 or not data.startswith(b"\x7fELF") for data in (binary, worker)):
        parser.error("expected a bounded ELF guest binary")
    files = {"init": (0o755, binary), "worker": (0o755, worker)}
    directories = {"dev", "proc", "sys"}
    if args.pdf_runtime:
        runtime, manifest = collect_pdf_runtime()
        files.update(runtime)
        files['pdf-runtime.json'] = (0o444, (json.dumps(manifest, sort_keys=True) + '\n').encode())
        for name in files:
            directories.update(str(parent) for parent in pathlib.PurePosixPath(name).parents
                               if str(parent) != '.')
    if args.video_runtime:
        from video_runtime import collect as collect_video_runtime
        runtime, manifest = collect_video_runtime()
        files.update(runtime)
        files['video-runtime.json'] = (0o444, (json.dumps(manifest, sort_keys=True) + '\n').encode())
        for name in files:
            directories.update(str(parent) for parent in pathlib.PurePosixPath(name).parents
                               if str(parent) != '.')
    if args.gif_runtime:
        from gif_runtime import collect as collect_gif_runtime
        runtime, manifest = collect_gif_runtime()
        files.update(runtime)
        files["gif-runtime.json"] = (0o444, (json.dumps(manifest, sort_keys=True) + "\n").encode())
    if args.still_runtime:
        from still_runtime import collect as collect_still_runtime
        runtime, manifest = collect_still_runtime()
        files.update(runtime)
        files["still-runtime.json"] = (0o444, (json.dumps(manifest, sort_keys=True) + "\n").encode())
        for name in files:
            directories.update(str(parent) for parent in pathlib.PurePosixPath(name).parents
                               if str(parent) != ".")
    image = bytearray()
    ino = 1
    for name in sorted(directories, key=lambda value: (value.count('/'), value)):
        image += entry(name, stat.S_IFDIR | 0o755, ino=ino)
        ino += 1
    contents, ino = file_entries(files, ino)
    image += contents
    image += entry("TRAILER!!!", 0, ino=ino)
    try:
        packaged = boot_image(image, pdf_runtime=args.pdf_runtime)
    except ValueError as error:
        parser.error(str(error))
    with args.output.open("xb") as output:
        output.write(packaged)
    print(f'Packaged {len(packaged)} bytes from {len(image)} unpacked bytes '
          f'(PDF runtime: {args.pdf_runtime}).')


if __name__ == "__main__":
    main()
