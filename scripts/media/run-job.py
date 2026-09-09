#!/usr/bin/env python3
"""Operator-only disposable Firecracker qualification runner.

This root-owned deployment utility has no service endpoint or database access.
Every argument is supplied by the local operator, never by a guest or web app.
The returned disk remains untrusted until board-media validates it.
"""
import argparse
import fcntl
import hashlib
import json
import os
import pathlib
import pwd
import shutil
import signal
import stat
import subprocess
import tempfile
import uuid

ENV = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C'}
OUTPUT_BYTES = 4_194_816
INPUT_BYTES = 8 * 1024 * 1024
JOBS = pathlib.Path('/run/26chan-media-jobs')


class Cancelled(RuntimeError):
    pass


def ignore_cancellation():
    for signum in (signal.SIGTERM, signal.SIGINT):
        signal.signal(signum, signal.SIG_IGN)


def cancel(signum, frame):
    # Repeated ordinary cancellation must not interrupt controlled unwinding.
    ignore_cancellation()
    raise Cancelled('job cancelled')


def command(args, **kwargs):
    return subprocess.run(args, env=ENV, check=True, stdin=subprocess.DEVNULL,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                          timeout=30, **kwargs)


def trusted_file(path, maximum):
    path = pathlib.Path(path)
    info = path.lstat()
    if (not path.is_absolute() or not stat.S_ISREG(info.st_mode) or info.st_uid != 0
            or info.st_mode & 0o022 or info.st_size > maximum):
        raise ValueError('artifact must be a bounded root-owned regular file')
    for parent in path.parents:
        metadata = parent.stat()
        # Root-owned sticky /tmp permits private root-owned artifact directories.
        if metadata.st_uid != 0 or (metadata.st_mode & 0o022 and not metadata.st_mode & stat.S_ISVTX):
            raise ValueError('artifact parent is not trusted')
    return path


def configuration(path):
    path = trusted_file(path, 4096)
    config = json.loads(path.read_bytes())
    if set(config) != {'firecracker', 'jailer', 'kernel', 'initramfs'}:
        raise ValueError('invalid artifact configuration')
    for name, artifact in config.items():
        if set(artifact) != {'path', 'sha256'}:
            raise ValueError('invalid artifact entry')
        source = trusted_file(artifact['path'], 64 * 1024 * 1024)
        with source.open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != artifact['sha256']:
                raise ValueError('artifact hash mismatch')
        config[name] = source
    for name in ('firecracker', 'jailer'):
        result = subprocess.run([str(config[name]), '--version'], env=ENV,
                                capture_output=True, timeout=5, check=True)
        if not result.stdout.splitlines() or result.stdout.splitlines()[0] != (name.capitalize() + ' v1.16.1').encode():
            raise ValueError('runtime version mismatch')
    return config


def input_disk(source, destination):
    descriptor = os.open(source, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, 'rb') as reader, destination.open('xb') as writer:
        info = os.fstat(reader.fileno())
        if not stat.S_ISREG(info.st_mode) or not 1 <= info.st_size <= INPUT_BYTES:
            raise ValueError('input size or file type rejected')
        writer.write(info.st_size.to_bytes(8, 'big'))
        remaining = info.st_size
        while remaining:
            data = reader.read(min(remaining, 8192))
            if not data:
                raise ValueError('input changed during intake')
            writer.write(data)
            remaining -= len(data)
        if reader.read(1):
            raise ValueError('input changed during intake')
        writer.write(bytes(-(info.st_size + 8) % 512))


def run(config, source, destination):
    user = pwd.getpwnam('board-media-vmm')
    if user.pw_uid == 0 or user.pw_gid == 0 or user.pw_shell != '/usr/sbin/nologin':
        raise ValueError('VMM identity rejected')
    JOBS.mkdir(mode=0o700, exist_ok=True)
    info = JOBS.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o077:
        raise ValueError('job root is not private')
    # This qualification profile permits one active VM. No shared writable job
    # collection is exposed, and the VMM identity is never reused concurrently.
    with (JOBS / 'runner.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        job_id = uuid.uuid4().hex
        unit = '26chan-media-' + job_id
        root = None
        output_reader = None
        try:
            # Block only the allocation-to-assignment window so cancellation
            # cannot lose the path that finally needs to remove.
            previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT})
            try:
                root = pathlib.Path(tempfile.mkdtemp(prefix=job_id + '-', dir=JOBS))
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, previous)
            command(['/usr/bin/mount', '-t', 'tmpfs', '-o', 'size=96M,nosuid,mode=0700',
                     'tmpfs', str(root)])
            shutil.copyfile(pathlib.Path(__file__).with_name('verify-cgroups.py'), root / 'verify-cgroups.py')
            (root / 'verify-cgroups.py').chmod(0o400)
            jail = root / 'firecracker' / job_id / 'root'
            jail.mkdir(parents=True, mode=0o700)
            for name in ('kernel', 'initramfs'):
                shutil.copyfile(config[name], jail / name)
                (jail / name).chmod(0o444)
            input_disk(source, jail / 'input.disk')
            (jail / 'input.disk').chmod(0o444)
            with (jail / 'output.disk').open('xb') as output:
                os.posix_fallocate(output.fileno(), 0, OUTPUT_BYTES)
            # Keep this exact inode open before the VMM starts. Never follow a
            # worker-controlled name or symlink when collecting the output.
            output_reader = (jail / 'output.disk').open('rb')
            os.chown(jail / 'output.disk', user.pw_uid, user.pw_gid)
            (jail / 'output.disk').chmod(0o600)
            machine = {
                'boot-source': {'kernel_image_path': '/kernel', 'initrd_path': '/initramfs',
                                'boot_args': 'console=ttyS0 reboot=k panic=1 pci=off rdinit=/init'},
                'drives': [
                    {'drive_id': 'input', 'path_on_host': '/input.disk', 'is_root_device': False,
                     'is_read_only': True},
                    {'drive_id': 'output', 'path_on_host': '/output.disk', 'is_root_device': False,
                     'is_read_only': False},
                ],
                'machine-config': {'vcpu_count': 1, 'mem_size_mib': 128, 'smt': False},
            }
            (jail / 'vm.json').write_text(json.dumps(machine))
            (jail / 'vm.json').chmod(0o444)
            # The service-side gate checks kernel controls before jailer runs.
            # It also pins jailer's parent selection to the verified membership.
            args = ['/usr/bin/systemd-run', '--quiet', '--wait', '--collect',
                    '--unit=' + unit, '--service-type=exec']
            for prop in ('MemoryMax=256M', 'MemorySwapMax=0', 'TasksMax=32', 'CPUQuota=100%',
                         'RuntimeMaxSec=15s', 'TimeoutStopSec=2s', 'KillMode=control-group',
                         'SendSIGKILL=yes', 'ExitType=cgroup', 'PrivateNetwork=yes',
                         'LimitCORE=0', 'LimitFSIZE=67108864', 'LimitNOFILE=64',
                         'StandardInput=null', 'StandardOutput=null', 'StandardError=null'):
                args.append('--property=' + prop)
            args += ['/usr/bin/python3', '-I', str(root / 'verify-cgroups.py'), unit + '.service',
                     str(config['jailer']), '--id', job_id, '--exec-file', str(config['firecracker']),
                     '--uid', str(user.pw_uid), '--gid', str(user.pw_gid),
                     '--chroot-base-dir', str(root), '--new-pid-ns',
                     '--resource-limit', 'fsize=4194816', '--resource-limit', 'no-file=64',
                     '--', '--no-api', '--config-file', '/vm.json']
            command(args)
            # A successful systemd exit is only a transport result. Do not parse
            # the guest's filesystem or trust its success, paths, or dimensions.
            try:
                if os.fstat(output_reader.fileno()).st_size != OUTPUT_BYTES:
                    raise ValueError('output device size changed')
                with open(destination, 'xb') as writer:
                    remaining = OUTPUT_BYTES
                    while remaining:
                        data = output_reader.read(min(remaining, 8192))
                        if not data:
                            raise ValueError('output device truncated')
                        writer.write(data)
                        remaining -= len(data)
                    if output_reader.read(1):
                        raise ValueError('output device oversized')
            finally:
                output_reader.close()
        finally:
            ignore_cancellation()
            if output_reader is not None:
                output_reader.close()
            # stop is synchronous and targets the full generated service. Never
            # remove its storage while a process may still have it open.
            subprocess.run(['/usr/bin/systemctl', 'stop', unit], env=ENV,
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                     timeout=10)
            state = subprocess.run(['/usr/bin/systemctl', 'is-active', unit], env=ENV,
                                   capture_output=True, timeout=5)
            if state.stdout.strip() not in (b'inactive', b'failed', b'unknown'):
                raise RuntimeError('service remains active; workspace retained')
            if root is not None:
                # mount may have completed immediately before cancellation.
                if os.path.ismount(root):
                    command(['/usr/bin/umount', str(root)])
                root.rmdir()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('config')
    parser.add_argument('input')
    parser.add_argument('output')
    args = parser.parse_args()
    if os.geteuid() != 0:
        parser.error('run only as an operator on an owned disposable Linux host')
    if os.environ.get('APP_ENV') == 'production':
        parser.error('this qualification runner is disabled in production')
    if any(name.endswith(('DATABASE_URL', 'TOKEN', 'SECRET', 'PASSWORD', 'ACCESS_KEY'))
           for name in os.environ):
        parser.error('remove credential-bearing environment variables')
    try:
        for signum in (signal.SIGTERM, signal.SIGINT):
            signal.signal(signum, cancel)
        run(configuration(args.config), args.input, args.output)
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
        parser.exit(1, 'isolated job failed; no validated publication produced\n')


if __name__ == '__main__':
    main()
