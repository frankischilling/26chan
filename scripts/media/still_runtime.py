"""Collect only the pinned operator-installed JPEG cleaner and C runtime."""
import hashlib
import pathlib
import re
import stat
import subprocess

PREFIX = pathlib.Path('/usr/lib/26chan-still')
MAX_FILE = 8 * 1024 * 1024
MAX_TOTAL = 16 * 1024 * 1024


def checked_bytes(path, *, elf=True):
    resolved = path.resolve(strict=True)
    roots = (PREFIX, pathlib.Path('/usr/lib'), pathlib.Path('/lib'), pathlib.Path('/lib64'))
    if not any(resolved.is_relative_to(root) for root in roots):
        raise ValueError('Still runtime path is outside fixed system roots')
    info = resolved.stat()
    if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0
            or info.st_mode & 0o022 or not 0 < info.st_size <= MAX_FILE):
        raise ValueError('Still runtime file is not bounded and root-owned')
    for parent in resolved.parents:
        metadata = parent.stat()
        if metadata.st_uid != 0 or metadata.st_mode & 0o022:
            raise ValueError('Still runtime parent is not trusted')
    data = resolved.read_bytes()
    if len(data) != info.st_size or (elf and not data.startswith(b'\x7fELF')):
        raise ValueError('Still runtime must be a bounded ELF file')
    return data


def dependencies(binary, env):
    result = subprocess.run(['/usr/bin/ldd', str(binary)], capture_output=True,
                            check=True, timeout=5, env=env).stdout.decode()
    paths = set(re.findall(r'(/[^\s()]+)', result))
    if 'not found' in result or not paths or len(paths) > 4:
        raise ValueError('Unexpected still runtime dependency set')
    files = {}
    for value in paths:
        if not re.fullmatch(r'(?:libc|libm)\.so\.\d+|ld-linux-x86-64\.so\.2', pathlib.Path(value).name):
            raise ValueError('Still runtime has an unreviewed dynamic dependency')
        files[value.removeprefix('/')] = (0o755, checked_bytes(pathlib.Path(value)))
    return files


def collect():
    binary = PREFIX / 'jpegtran'
    exif = PREFIX / 'exiftags'
    files = {'usr/bin/board-jpegtran': (0o755, checked_bytes(binary)),
             'usr/bin/board-exiftags': (0o755, checked_bytes(exif))}
    notice = checked_bytes(PREFIX / 'exiftags-copyright', elf=False)
    if hashlib.sha256(notice).hexdigest() != 'ec4bfb420f82792b7947d38619fa265321b1764d0766502370cf00bd8ba0d870':
        raise ValueError('EXIF copyright notice differs from the pinned release')
    files['usr/share/doc/26chan-exiftags/copyright'] = (0o644, notice)
    env = {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
    version = subprocess.run([str(binary), '-version'], capture_output=True,
                             check=True, timeout=5, env=env)
    if not (version.stdout + version.stderr).startswith(b'libjpeg-turbo version 3.2.0 '):
        raise ValueError('Unexpected still runtime version')
    version = subprocess.run([str(exif), '-h'], capture_output=True,
                             check=False, timeout=5, env=env)
    if version.returncode != 1 or b'\nVersion: 1.01\n' not in version.stderr or version.stdout:
        raise ValueError('Unexpected EXIF runtime version')
    for executable in (binary, exif):
        for name, data in dependencies(executable, env).items():
            if name in files and files[name] != data:
                raise ValueError('Inconsistent still runtime dependency')
            files[name] = data
    if sum(len(data) for _, data in files.values()) > MAX_TOTAL:
        raise ValueError('Still runtime exceeds its image budget')
    return files, {name: hashlib.sha256(data).hexdigest() for name, (_, data) in sorted(files.items())}
