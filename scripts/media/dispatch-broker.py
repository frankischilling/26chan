#!/usr/bin/env python3
"""Development-only root launcher for one UID-authenticated local media slot."""
import contextlib
import fcntl
import importlib.util
import itertools
import os
import pathlib
import pwd
import re
import signal
import socket
import stat
import struct
import subprocess
import sys
import uuid

from dispatch_protocol import receive_request, send_response
from job_lifecycle import locked_jobs, reconcile_jobs

spec = importlib.util.spec_from_file_location('media_runner', pathlib.Path(__file__).with_name('run-job.py'))
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
REQUEST = re.compile(r'request-[0-9a-f]{32}')
SIGNALS = {signal.SIGTERM, signal.SIGINT}
# Require the operator's cleared environment, including Python's optional
# locale coercion. Credential-file variables need no naming heuristic.
ALLOWED_ENV = {'APP_ENV', 'PATH', 'LANG', 'LC_ALL', 'LC_CTYPE', 'TZ'}


class RequestRetained(RuntimeError):
    """Cleanup could not establish that all managed processes have stopped."""


def reset_cancellation():
    for signum in SIGNALS:
        signal.signal(signum, runner.cancel)


def trusted_directory(path, mode, gid=0):
    info = path.lstat()
    if (not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_gid != gid
            or stat.S_IMODE(info.st_mode) != mode):
        raise ValueError('dispatch directory rejected')


def mounted_paths():
    # mountinfo escapes whitespace and backslashes as octal sequences.
    return {pathlib.Path(re.sub(r'\\([0-7]{3})', lambda m: chr(int(m[1], 8)), line.split()[4]))
            for line in pathlib.Path('/proc/self/mountinfo').read_text().splitlines()}


def verify_request(path, mounts):
    if REQUEST.fullmatch(path.name) is None:
        raise ValueError('dispatch request name rejected')
    trusted_directory(path, 0o700)
    if any(mount == path or path in mount.parents for mount in mounts):
        raise ValueError('dispatch request mount rejected')
    entries = list(itertools.islice(path.iterdir(), 3))
    if len(entries) > 2:
        raise ValueError('dispatch request entries rejected')
    for entry in entries:
        info = entry.lstat()
        if (entry.name not in ('input', 'output') or not stat.S_ISREG(info.st_mode)
                or info.st_uid != 0 or info.st_gid != 0 or info.st_nlink != 1
                or stat.S_IMODE(info.st_mode) != 0o600):
            raise ValueError('dispatch request file rejected')
    return entries


def remove_request(path):
    for entry in verify_request(path, mounted_paths()):
        entry.unlink()
    path.rmdir()


def recover_requests(requests):
    trusted_directory(requests, 0o700)
    mounts = mounted_paths()
    if requests in mounts:
        raise ValueError('dispatch storage mount rejected')
    entries = list(itertools.islice(requests.iterdir(), 17))
    if len(entries) > 16:
        raise ValueError('dispatch recovery inventory rejected')
    # Validate everything before deleting any request.
    for entry in entries:
        verify_request(entry, mounts)
    for entry in entries:
        remove_request(entry)


def handle_connection(connection, allowed_uid, requests, execute):
    _, uid, _ = struct.unpack('3i', connection.getsockopt(
        socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize('3i')))
    if uid != allowed_uid:
        raise PermissionError('dispatch caller rejected')
    request = None
    retain = False
    try:
        previous = signal.pthread_sigmask(signal.SIG_BLOCK, SIGNALS)
        try:
            candidate = requests / ('request-' + uuid.uuid4().hex)
            candidate.mkdir(mode=0o700)
            request = candidate
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous)
        receive_request(connection, request / 'input')
        execute(request / 'input', request / 'output')
        send_response(connection, request / 'output')
    except RequestRetained:
        retain = True
        raise
    finally:
        if request is not None and not retain:
            # A pending ordinary cancellation cannot interrupt verified removal.
            previous = signal.pthread_sigmask(signal.SIG_BLOCK, SIGNALS)
            try:
                try:
                    remove_request(request)
                except (OSError, ValueError):
                    raise RequestRetained('dispatch request cleanup incomplete') from None
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, previous)


@contextlib.contextmanager
def broker_lock(directory, gateway):
    if not directory.is_absolute() or '..' in directory.parts:
        raise ValueError('dispatch socket directory rejected')
    for parent in directory.parents:
        info = parent.lstat()
        if (not stat.S_ISDIR(info.st_mode) or info.st_uid != 0
                or (info.st_mode & 0o022 and not info.st_mode & stat.S_ISVTX)):
            raise ValueError('dispatch socket parent rejected')
    try:
        directory.mkdir(mode=0o700)
    except FileExistsError:
        pass
    else:
        os.chown(directory, 0, gateway.pw_gid)
        directory.chmod(0o750)
    trusted_directory(directory, 0o750, gateway.pw_gid)
    if directory in mounted_paths():
        raise ValueError('dispatch socket mount rejected')
    fd = os.open(directory / 'broker.lock', os.O_RDWR | os.O_CREAT | os.O_CLOEXEC |
                 os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    with os.fdopen(fd, 'r+') as lock:
        info = os.fstat(lock.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_gid != 0
                or info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600):
            raise ValueError('dispatch lock rejected')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


def serve(config, directory, gateway):
    with broker_lock(directory, gateway):
        requests = directory / 'requests'
        requests.mkdir(mode=0o700, exist_ok=True)
        with locked_jobs():
            reconcile_jobs()
            recover_requests(requests)
        endpoint = directory / 'broker.sock'
        if os.path.lexists(endpoint):
            info = endpoint.lstat()
            if not stat.S_ISSOCK(info.st_mode) or info.st_uid != 0 or endpoint in mounted_paths():
                raise ValueError('dispatch stale socket rejected')
            endpoint.unlink()

        def execute(source, destination):
            try:
                runner.run(config, source, destination)
            except BaseException:
                # A failed stop must retain input/output as well as VM storage.
                try:
                    with locked_jobs():
                        reconcile_jobs()
                except BaseException:
                    raise RequestRetained('dispatch cleanup incomplete') from None
                raise
            finally:
                reset_cancellation()

        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
            listener.bind(str(endpoint))
            try:
                os.chown(endpoint, 0, gateway.pw_gid)
                endpoint.chmod(0o660)
                listener.listen(1)
                while True:
                    reset_cancellation()
                    connection, _ = listener.accept()
                    with connection:
                        try:
                            handle_connection(connection, gateway.pw_uid, requests, execute)
                        except (runner.Cancelled, RequestRetained):
                            raise
                        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
                            print('dispatch request rejected', file=sys.stderr)
            finally:
                runner.ignore_cancellation()
                endpoint.unlink()


def main():
    try:
        if len(sys.argv) != 4 or os.geteuid() != 0 or os.environ.get('APP_ENV') != 'development':
            raise ValueError('dispatch startup rejected')
        if set(os.environ) - ALLOWED_ENV:
            raise ValueError('dispatch environment rejected')
        gateway = pwd.getpwuid(int(sys.argv[3]))
        vmm = pwd.getpwnam('board-media-vmm')
        if (gateway.pw_uid in (0, vmm.pw_uid) or gateway.pw_gid == 0
                or gateway.pw_shell != '/usr/sbin/nologin'):
            raise ValueError('dispatch gateway identity rejected')
        os.umask(0o077)
        reset_cancellation()
        serve(runner.configuration(sys.argv[1]), pathlib.Path(sys.argv[2]), gateway)
    except runner.Cancelled:
        return 0
    except (OSError, ValueError, RuntimeError, KeyError, subprocess.SubprocessError):
        print('dispatch broker failed', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
