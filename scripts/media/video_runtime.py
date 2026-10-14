"""Package the fixed operator-installed minimal video tools and libc runtime.

The build recipe pins the signed upstream source and disables network protocols,
devices, external libraries and unused codecs. No upload reaches this collector.
"""
import hashlib
import pathlib
import re
import stat
import subprocess

PREFIX = pathlib.Path('/usr/lib/26chan-video')
MAX_TOTAL = 24 * 1024 * 1024
MAX_FILE = 16 * 1024 * 1024


def checked_bytes(path):
    resolved = path.resolve(strict=True)
    roots = (PREFIX, pathlib.Path('/usr/lib'), pathlib.Path('/lib'), pathlib.Path('/lib64'))
    if not any(resolved.is_relative_to(root) for root in roots):
        raise ValueError('Video runtime path is outside fixed system roots')
    info = resolved.stat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022 or not 0 < info.st_size <= MAX_FILE:
        raise ValueError('Video runtime file is not bounded and root-owned')
    for parent in resolved.parents:
        metadata = parent.stat()
        if metadata.st_uid != 0 or metadata.st_mode & 0o022:
            raise ValueError(f'Video runtime parent is not trusted: {parent}')
    data = resolved.read_bytes()
    if len(data) != info.st_size or not data.startswith(b'\x7fELF'):
        raise ValueError('Video runtime must be a bounded ELF file')
    return data


def collect():
    files = {}
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    for name in ('ffmpeg', 'ffprobe'):
        binary = PREFIX / name
        files['usr/bin/board-' + name] = (0o755, checked_bytes(binary))
        version = subprocess.run([str(binary), '-version'], capture_output=True,
                                 check=True, timeout=5, env=env).stdout
        if not version.startswith((name + ' version 7.1.5 ').encode()):
            raise ValueError('Unexpected video runtime version')
        protocols = subprocess.run([str(binary), '-v', 'quiet', '-protocols'], capture_output=True,
                                   check=True, timeout=5, env=env).stdout.decode().splitlines()
        if {line.strip() for line in protocols if line.startswith('  ')} != {'file', 'pipe'}:
            raise ValueError('Video runtime protocols exceed the reviewed build')
        libraries = subprocess.run(['/usr/bin/ldd', str(binary)], capture_output=True,
                                   check=True, timeout=5, env=env).stdout.decode()
        paths = set(re.findall(r'(/[^\s()]+)', libraries))
        if 'not found' in libraries or not paths or len(paths) > 8:
            raise ValueError('Unexpected video dependency set')
        for value in paths:
            # Minimal static FFmpeg links only the platform C/math/thread/runtime
            # libraries. Reject distro builds with unrelated parser dependencies.
            if not re.fullmatch(r'(?:libc|libm|libpthread|libdl|librt)\.so\.\d+|ld-linux-x86-64\.so\.2', pathlib.Path(value).name):
                raise ValueError('Video runtime has an unreviewed dynamic dependency')
            files[value.removeprefix('/')] = (0o755, checked_bytes(pathlib.Path(value)))
    if sum(len(data) for _, data in files.values()) > MAX_TOTAL:
        raise ValueError('Video runtime exceeds its image budget')
    return files, {name: hashlib.sha256(data).hexdigest() for name, (_, data) in sorted(files.items())}
