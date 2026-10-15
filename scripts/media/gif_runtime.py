"""Collect only the pinned operator-installed GIF cleaner and C runtime."""
import hashlib
import pathlib
import re
import stat
import subprocess

PREFIX = pathlib.Path('/usr/lib/26chan-gif')
MAX_FILE = 8 * 1024 * 1024
MAX_TOTAL = 16 * 1024 * 1024


def checked_bytes(path):
    resolved = path.resolve(strict=True)
    roots = (PREFIX, pathlib.Path('/usr/lib'), pathlib.Path('/lib'), pathlib.Path('/lib64'))
    if not any(resolved.is_relative_to(root) for root in roots):
        raise ValueError('GIF runtime path is outside fixed system roots')
    info = resolved.stat()
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0
            or info.st_mode & 0o022 or not 0 < info.st_size <= MAX_FILE):
        raise ValueError('GIF runtime file is not bounded and root-owned')
    for parent in resolved.parents:
        metadata = parent.stat()
        if metadata.st_uid != 0 or metadata.st_mode & 0o022:
            raise ValueError('GIF runtime parent is not trusted')
    data = resolved.read_bytes()
    if len(data) != info.st_size or not data.startswith(b'\x7fELF'):
        raise ValueError('GIF runtime must be a bounded ELF file')
    return data


def collect():
    binary = PREFIX / 'gifsicle'
    files = {'usr/bin/board-gifsicle': (0o755, checked_bytes(binary))}
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    version = subprocess.run([str(binary), '--version'], capture_output=True,
                             check=True, timeout=5, env=env)
    if not (version.stdout + version.stderr).startswith(b'LCDF Gifsicle 1.96\n'):
        raise ValueError('Unexpected GIF runtime version')
    result = subprocess.run(['/usr/bin/ldd', str(binary)], capture_output=True,
                            check=True, timeout=5, env=env).stdout.decode()
    paths = set(re.findall(r'(/[^\s()]+)', result))
    if 'not found' in result or not paths or len(paths) > 4:
        raise ValueError('Unexpected GIF runtime dependency set')
    for value in paths:
        if not re.fullmatch(r'(?:libc|libm)\.so\.\d+|ld-linux-x86-64\.so\.2', pathlib.Path(value).name):
            raise ValueError('GIF runtime has an unreviewed dynamic dependency')
        files[value.removeprefix('/')] = (0o755, checked_bytes(pathlib.Path(value)))
    if sum(len(data) for _, data in files.values()) > MAX_TOTAL:
        raise ValueError('GIF runtime exceeds its image budget')
    return files, {name: hashlib.sha256(data).hexdigest() for name, (_, data) in sorted(files.items())}
