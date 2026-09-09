#!/usr/bin/env python3
"""Prepare pinned artifacts for owned disposable Linux qualification only."""
import argparse
import hashlib
import json
import os
import pathlib
import pwd
import subprocess
import tarfile
import urllib.request

REPO = pathlib.Path(__file__).resolve().parents[2]


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=pathlib.Path)
    parser.add_argument('binary_directory', type=pathlib.Path)
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error('requires root on an owned disposable Linux test host')
    target = args.directory.absolute()
    target.mkdir(mode=0o700)  # Refuse reuse of any earlier artifact collection.
    manifest = json.loads((REPO / 'docs/firecracker-artifacts.json').read_text())
    for item in manifest['files']:
        destination = target / item['name']
        with urllib.request.urlopen(item['url'], timeout=30) as response, destination.open('xb') as output:
            remaining = 64 * 1024 * 1024
            while data := response.read(min(8192, remaining + 1)):
                remaining -= len(data)
                if remaining < 0:
                    raise ValueError('artifact exceeds size limit')
                output.write(data)
        if digest(destination) != item['sha256']:
            raise ValueError('artifact checksum mismatch')
    with tarfile.open(target / 'firecracker.tgz') as archive:
        for name in ('firecracker', 'jailer'):
            member = archive.getmember(f'release-v1.16.1-x86_64/{name}-v1.16.1-x86_64')
            if not member.isfile() or member.size > 32 * 1024 * 1024:
                raise ValueError('invalid release executable')
            with (target / name).open('xb') as output, archive.extractfile(member) as source:
                output.write(source.read())
            (target / name).chmod(0o755)
    try:
        user = pwd.getpwnam('board-media-vmm')
        if user.pw_uid == 0 or user.pw_shell != '/usr/sbin/nologin':
            raise ValueError('existing VMM account has unexpected privileges')
    except KeyError:
        subprocess.run(['/usr/sbin/useradd', '--system', '--no-create-home', '--shell',
                        '/usr/sbin/nologin', 'board-media-vmm'], check=True)
    binaries = args.binary_directory.absolute()
    for name, worker in [('decode', binaries / 'media-decode'),
                         ('probe', binaries / 'examples/containment-probe')]:
        initramfs = target / f'{name}.initramfs'
        subprocess.run(['/usr/bin/python3', str(REPO / 'scripts/media/build-initramfs.py'),
                        str(binaries / 'board-media-guest'), str(worker), str(initramfs)], check=True)
        config = {}
        for key, path in [('firecracker', target / 'firecracker'), ('jailer', target / 'jailer'),
                          ('kernel', target / 'vmlinux'), ('initramfs', initramfs)]:
            config[key] = {'path': str(path), 'sha256': digest(path)}
        destination = target / f'{name}.json'
        destination.write_text(json.dumps(config, indent=2) + '\n')
        destination.chmod(0o600)
    print(f'Prepared test artifacts at {target}; production qualification remains required.')


if __name__ == '__main__':
    main()
