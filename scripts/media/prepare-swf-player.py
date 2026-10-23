#!/usr/bin/env python3
"""Prepare pinned self-hosted Ruffle assets with a fixed WASM memory maximum.

The output is a new directory, never an overwrite of an installed runtime.
This bounds WASM linear memory, not total browser, JavaScript or GPU memory.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path
import urllib.request
import zipfile

VERSION = '0.7.1'
URL = f'https://github.com/ruffle-rs/ruffle/releases/download/v{VERSION}/ruffle-{VERSION}-web-selfhosted.zip'
ARCHIVE_SHA256 = 'ef12edbee135d581a9cd523837d746da30a07a64dbf1cf472d2e472c656f4b5e'
MAX_PAGES = 4096  # 256 MiB, enforced by the WebAssembly engine.
FILES = ('ruffle.js', 'core.ruffle.16d61072ac7fdc4d5013.js',
         'core.ruffle.994eeef0a471a88cd0e3.js', '172de42e6619eac8f371.wasm',
         '878d548199cddef07035.wasm', 'LICENSE_MIT', 'LICENSE_APACHE')


def uleb(data, offset):
    value = 0
    for count in range(5):
        if offset >= len(data):
            raise ValueError('truncated WebAssembly integer')
        byte = data[offset]
        offset += 1
        value |= (byte & 127) << (7 * count)
        if byte < 128:
            if value > 0xffffffff:
                raise ValueError('WebAssembly integer overflow')
            return value, offset
    raise ValueError('oversized WebAssembly integer')


def encode(value):
    output = bytearray()
    while value >= 128:
        output.append((value & 127) | 128)
        value >>= 7
    output.append(value)
    return bytes(output)


def cap_memory(data):
    if data[:8] != b'\0asm\x01\0\0\0':
        raise ValueError('unexpected WebAssembly header')
    output = bytearray(data[:8])
    offset = 8
    found = False
    while offset < len(data):
        section = data[offset]
        length, start = uleb(data, offset + 1)
        end = start + length
        if end > len(data):
            raise ValueError('truncated WebAssembly section')
        body = data[start:end]
        if section == 5:
            if found:
                raise ValueError('duplicate memory section')
            count, cursor = uleb(body, 0)
            flags, cursor = uleb(body, cursor)
            minimum, cursor = uleb(body, cursor)
            # Assert the exact reviewed module shape before changing limits.
            if count != 1 or flags != 0 or minimum != 73 or cursor != len(body):
                raise ValueError('unexpected Ruffle memory declaration')
            body = encode(1) + encode(1) + encode(minimum) + encode(MAX_PAGES)
            found = True
        output.append(section)
        output.extend(encode(len(body)))
        output.extend(body)
        offset = end
    if not found:
        raise ValueError('Ruffle memory declaration missing')
    return bytes(output)


def prepare(archive, destination):
    if hashlib.sha256(archive).hexdigest() != ARCHIVE_SHA256:
        raise ValueError('Ruffle archive digest mismatch')
    files = {}
    with zipfile.ZipFile(io.BytesIO(archive)) as source:
        if len(source.namelist()) != len(set(source.namelist())):
            raise ValueError('duplicate archive entries')
        for name in FILES:
            entry = source.getinfo(name)
            if entry.file_size > 16 * 1024 * 1024:
                raise ValueError('asset exceeds reviewed limit')
            data = source.read(entry)
            files[name] = cap_memory(data) if name.endswith('.wasm') else data
    manifest = {'version': VERSION, 'archive_url': URL, 'archive_sha256': ARCHIVE_SHA256,
                'profile': 'ruffle-0.7.1-memory-cap-v1', 'wasm_max_pages': MAX_PAGES,
                'files': {name: {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
                          for name, data in files.items()}}
    # All source validation precedes filesystem changes.
    destination.mkdir(parents=False, exist_ok=False)
    for name, data in files.items():
        (destination / name).write_bytes(data)
    (destination / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--archive', type=Path)
    args = parser.parse_args()
    if args.archive:
        with args.archive.open('rb') as source:
            archive = source.read(16 * 1024 * 1024 + 1)
    else:
        with urllib.request.urlopen(URL, timeout=60) as source:
            archive = source.read(16 * 1024 * 1024 + 1)
    if len(archive) > 16 * 1024 * 1024:
        raise ValueError('archive exceeds reviewed limit')
    manifest = prepare(archive, args.destination)
    print(f"Prepared {manifest['profile']} in {args.destination}")


if __name__ == '__main__':
    main()
