"""Run one operator-configured maintenance command and durably record its outcome."""

from contextlib import ExitStack
import importlib.util
import json
import os
from pathlib import Path
import secrets
import signal
import stat
import subprocess
import sys
import time

ERROR = 'Maintenance recording failed.'

# Support `python3 -I .../run.py CONFIG`: isolated mode omits the script directory
# from sys.path. This sibling belongs to the same trusted operator installation.
try:
    _spec = importlib.util.spec_from_file_location('maintenance_state', Path(__file__).resolve().with_name('state.py'))
    state = importlib.util.module_from_spec(_spec)
    _spec.loader.exec_module(state)
except Exception:
    print(ERROR, file=sys.stderr)
    raise SystemExit(1) from None

CHILD_ENVIRONMENT = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C'}


def _directory_policy(metadata, *, final=False):
    uid = os.geteuid()
    if not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid not in ((uid,) if final else (0, uid)):
        raise ValueError(ERROR)
    if metadata.st_mode & 0o022:
        # Explicit narrow ancestor exception: root-owned sticky /tmp-style
        # directories protect entries by ownership. The final state directory
        # never receives this exception, and every component is still NOFOLLOW.
        if final or metadata.st_uid != 0 or not metadata.st_mode & stat.S_ISVTX:
            raise ValueError(ERROR)


def open_directory(path, *, final=False):
    state.canonical_path(path)
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
    descriptor = os.open('/', flags)
    try:
        _directory_policy(os.fstat(descriptor))
        for component in path.split('/')[1:]:
            if not component:
                continue
            following = os.open(component, flags, dir_fd=descriptor)
            os.close(descriptor)
            descriptor = following
            _directory_policy(os.fstat(descriptor))
        if final:
            _directory_policy(os.fstat(descriptor), final=True)
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def _regular_policy(metadata, *, executable=False):
    if (not stat.S_ISREG(metadata.st_mode) or metadata.st_uid not in ((0, os.geteuid()) if executable else (os.geteuid(),))
            or metadata.st_mode & 0o022):
        raise ValueError(ERROR)


def open_file(path, *, executable=False):
    state.canonical_path(path)
    parent, name = path.rsplit('/', 1)
    if not name:
        raise ValueError(ERROR)
    directory = open_directory(parent or '/')
    try:
        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=directory)
    finally:
        os.close(directory)
    try:
        _regular_policy(os.fstat(descriptor), executable=executable)
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def read_bounded(descriptor, limit):
    if os.fstat(descriptor).st_size > limit:
        raise ValueError(ERROR)
    data = bytearray()
    while len(data) <= limit:
        chunk = os.read(descriptor, limit + 1 - len(data))
        if not chunk:
            return bytes(data)
        data.extend(chunk)
    raise ValueError(ERROR)


class Journal:
    def __init__(self, directory, target):
        if target not in state.TARGETS:
            raise ValueError(ERROR)
        self.target = target
        self.directory = open_directory(directory, final=True)
        self.lock = None

    def __enter__(self):
        import fcntl
        try:
            self.lock = os.open(self.target + '.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                                0o600, dir_fd=self.directory)
            _regular_policy(os.fstat(self.lock))
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            os.fchmod(self.lock, 0o600)
            return self
        except BaseException:
            self.__exit__()
            raise

    def __exit__(self, *_exception):
        if self.lock is not None:
            os.close(self.lock)
            self.lock = None
        if self.directory is not None:
            os.close(self.directory)
            self.directory = None

    def read(self):
        try:
            descriptor = os.open(self.target + '.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC,
                                 dir_fd=self.directory)
        except FileNotFoundError:
            return None
        try:
            _regular_policy(os.fstat(descriptor))
            return state.parse_journal(read_bounded(descriptor, state.JOURNAL_LIMIT), self.target)
        finally:
            os.close(descriptor)

    def publish(self, value):
        state.validate_journal(value, self.target)
        data = (json.dumps(value, separators=(',', ':')) + '\n').encode('utf-8')
        if len(data) > state.JOURNAL_LIMIT:
            raise ValueError(ERROR)
        temporary = '.' + self.target + '.' + secrets.token_hex(16) + '.tmp'
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                             0o644, dir_fd=self.directory)
        try:
            with os.fdopen(descriptor, 'wb') as output:
                # A restrictive service umask must not hide the nonsensitive
                # journal from the separate read-only observer identity.
                os.fchmod(output.fileno(), 0o644)
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary, self.target + '.json', src_dir_fd=self.directory, dst_dir_fd=self.directory)
            os.fsync(self.directory)
        finally:
            try:
                os.unlink(temporary, dir_fd=self.directory)
            except FileNotFoundError:
                pass


class StopRequest:
    def __init__(self):
        self.signum = 0

    def receive(self, signum, _frame):
        # Defer action to bounded control points instead of interrupting an
        # atomic journal publication or dropping ownership halfway through spawn.
        if not self.signum:
            self.signum = signum


def stop_group_and_reap(child):
    # Do not call Popen.poll()/wait() or a reaping waitid before these signals.
    # An exited leader remains our unreaped child, pinning its PID/group number.
    # If ownership was unexpectedly lost, refuse to signal a number that could
    # already have been reused. This CLI is single-threaded and resets SIGCHLD,
    # so a successful nonreaping wait pins ownership until our final wait().
    os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
    try:
        os.killpg(child.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    deadline = time.monotonic() + 1
    while time.monotonic() < deadline:
        time.sleep(min(0.05, max(0, deadline - time.monotonic())))
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    return child.wait(timeout=5)


def run_command(config, executable, stop):
    if stop.signum:
        return False
    child = None
    finished_ok = False
    try:
        deadline = time.monotonic() + config['timeout_seconds']
        # Execute the descriptor validated without following source symlinks.
        # Only this nonsensitive executable FD is inherited; no journal/lock FD
        # or caller environment reaches the configured maintenance command.
        child = subprocess.Popen(config['command'], executable='/proc/self/fd/' + str(executable),
                                 pass_fds=(executable,), cwd='/', env=CHILD_ENVIRONMENT, shell=False,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 start_new_session=True)
        while not stop.signum and time.monotonic() < deadline:
            observed = os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
            if observed is not None:
                finished_ok = observed.si_code == os.CLD_EXITED and observed.si_status == 0
                break
            time.sleep(0.02)
    except OSError:
        finished_ok = False
    finally:
        if child is not None:
            reaped = stop_group_and_reap(child)
            finished_ok = finished_ok and reaped == 0
    return finished_ok and not stop.signum


def finalize(journal, running, succeeded, stop):
    termination = (signal.SIGTERM, signal.SIGINT)
    previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK, termination)
    try:
        pending = signal.sigpending()
        signum = stop.signum or next((number for number in termination if number in pending), 0)
        succeeded = succeeded and not signum
        # Refuse wall-clock rollback instead of inventing a completion time.
        finished_ms = time.time_ns() // 1_000_000
        journal.publish(state.finish_attempt(running, succeeded, finished_ms))
        pending = signal.sigpending()
        signum = signum or stop.signum or next((number for number in termination if number in pending), 0)
        if succeeded and signum:
            journal.publish(state.finish_attempt(running, False, finished_ms))
            succeeded = False
        # The final pending-signal check is this dedicated CLI's outcome commit
        # point. Keep termination blocked through descriptor/handler teardown,
        # status output, and process exit: later signals cannot change its exit
        # status after the corresponding durable journal has been committed.
        return 128 + signum if signum else (0 if succeeded else 1)
    except BaseException:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
        raise


def record(config_path):
    if not sys.platform.startswith('linux') or sys.version_info < (3, 12):
        raise ValueError(ERROR)
    stop = StopRequest()
    with ExitStack() as cleanup:
        previous_child = signal.signal(signal.SIGCHLD, signal.SIG_DFL)
        cleanup.callback(signal.signal, signal.SIGCHLD, previous_child)
        for signum in (signal.SIGTERM, signal.SIGINT):
            previous = signal.signal(signum, stop.receive)
            cleanup.callback(signal.signal, signum, previous)
        descriptor = open_file(config_path)
        try:
            config = state.parse_config(read_bounded(descriptor, state.CONFIG_LIMIT))
        finally:
            os.close(descriptor)
        executable = open_file(config['command'][0], executable=True)
        cleanup.callback(os.close, executable)
        journal = cleanup.enter_context(Journal(config['state_directory'], config['target']))
        running = state.start_attempt(config['target'], journal.read(), time.time_ns() // 1_000_000)
        journal.publish(running)
        succeeded = run_command(config, executable, stop)
        return finalize(journal, running, succeeded, stop)


def main():
    try:
        if len(sys.argv) != 2:
            raise ValueError(ERROR)
        result = record(sys.argv[1])
        print('Maintenance command recorded.' if result == 0 else ERROR, file=sys.stdout if result == 0 else sys.stderr)
        return result
    except Exception:
        print(ERROR, file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
