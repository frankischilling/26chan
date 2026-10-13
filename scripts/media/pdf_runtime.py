"""Collect the installed, root-owned Ghostscript runtime for an isolated guest.

Only the fixed system executable is inspected. No upload or supplied executable
is passed to ldd, and no host credentials or general system configuration enter
the image. The resulting initramfs digest remains part of the operator manifest.
"""
import hashlib
import json
import pathlib
import os
import re
import stat
import subprocess

EXECUTABLE = pathlib.Path('/usr/bin/gs')
DATA_ROOTS = (
    pathlib.Path('/usr/share/ghostscript'),
    pathlib.Path('/usr/share/fonts/type1/urw-base35'),
    pathlib.Path('/usr/share/color/icc/ghostscript'),
)
MAX_FILE = 32 * 1024 * 1024
MAX_TOTAL = 60 * 1024 * 1024
MAX_FILES = 2048
# Resource packages have reviewed content pins because some CI images make
# /usr/share writable. These pins never apply to executables or shared libraries.
RESOURCE_MANIFEST = pathlib.Path(__file__).resolve().parents[2] / 'docs/pdf-runtime-resources.json'
PINNED_RESOURCES = {
    pathlib.Path(name): (value['size'], value['sha256'])
    for name, value in json.loads(RESOURCE_MANIFEST.read_text())['files'].items()
}


def pinned_resource_bytes(path, expected):
    size, digest = expected
    # Read and hash the same bounded bytes that enter the image. A path or
    # directory replacement cannot substitute different bytes after validation.
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size != size or size > MAX_FILE:
            raise ValueError('Pinned PDF resource is not a bounded regular file')
        data = stream.read(size + 1)
    if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
        raise ValueError('Pinned PDF resource differs from the reviewed package')
    return data


def checked_bytes(path, *, resource=False):
    resolved = path.resolve(strict=True)
    allowed = (pathlib.Path('/usr'), pathlib.Path('/lib'), pathlib.Path('/lib64'))
    if not any(resolved.is_relative_to(root) for root in allowed):
        raise ValueError('PDF runtime source is outside the fixed system roots')
    info = resolved.stat()
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0
            or info.st_mode & 0o022 or not 0 <= info.st_size <= MAX_FILE):
        raise ValueError('PDF runtime source is not a bounded root-owned file')
    if resource and path in PINNED_RESOURCES:
        return pinned_resource_bytes(resolved, PINNED_RESOURCES[path])
    for parent in resolved.parents:
        metadata = parent.stat()
        if metadata.st_uid != 0 or metadata.st_mode & 0o022:
            raise ValueError(f'PDF runtime source directory is not trusted: {parent} '
                             f'(uid={metadata.st_uid}, mode={stat.S_IMODE(metadata.st_mode):04o}) '
                             f'for {resolved}')
    return resolved.read_bytes()


def collect():
    files = {'usr/bin/gs': (0o755, checked_bytes(EXECUTABLE))}
    if not files['usr/bin/gs'][1].startswith(b'\x7fELF'):
        raise ValueError('Ghostscript must be the installed ELF executable')
    result = subprocess.run(
        ['/usr/bin/ldd', str(EXECUTABLE)], check=True, capture_output=True,
        text=True, timeout=10, env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'},
    )
    if 'not found' in result.stdout:
        raise ValueError('Ghostscript has an unresolved library')
    paths = set(re.findall(r'(/[^\s()]+)', result.stdout))
    if not paths or len(paths) > 128:
        raise ValueError('Unexpected Ghostscript dependency set')
    for value in sorted(paths):
        path = pathlib.Path(value)
        if not (value.startswith('/lib/') or value.startswith('/lib64/')
                or value.startswith('/usr/lib/')):
            raise ValueError('Unexpected Ghostscript library location')
        data = checked_bytes(path)
        if not data.startswith(b'\x7fELF'):
            raise ValueError('Ghostscript dependency must be ELF')
        files[value[1:]] = (0o755, data)
    for root in DATA_ROOTS:
        if not root.is_dir():
            raise ValueError('Required Ghostscript data package is absent')
        for path in sorted(root.rglob('*')):
            if path.is_file():
                files[str(path)[1:]] = (0o644, checked_bytes(path, resource=True))
                if len(files) > MAX_FILES:
                    raise ValueError('PDF runtime has too many files')
    # Font selection is limited to the installed base fonts; neither the host's
    # fontconfig configuration nor its caches are copied into the guest.
    if sum(len(data) for _, data in files.values()) > MAX_TOTAL:
        raise ValueError('PDF runtime exceeds the guest image budget')
    manifest = {name: hashlib.sha256(data).hexdigest()
                for name, (_, data) in sorted(files.items())}
    return files, manifest
