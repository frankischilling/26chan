"""Fixed-namespace lifecycle for operator-owned media jobs; never publishes output."""
import contextlib
import fcntl
import os
import pathlib
import pwd
import re
import signal
import stat
import subprocess
import time

ENV = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C'}
JOBS = pathlib.Path('/run/26chan-media-jobs')
UNIT = re.compile(r'26chan-media-[0-9a-f]{32}\.service')
WORKSPACE = re.compile(r'([0-9a-f]{32})-[a-z0-9_]{8}')
MAX_ORPHANS = 16


class JobProcessesRemain(RuntimeError):
    pass


def run_service(args, lock, *, deadline=30):
    """Bound the trusted launch client independently of the Python caller."""
    process = None
    try:
        previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT})
        try:
            process = subprocess.Popen(
                ['/usr/bin/timeout', '--signal=TERM', '--kill-after=2s', str(deadline) + 's', *args],
                env=ENV, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, start_new_session=True, pass_fds=(lock.fileno(),))
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous)
        result = process.wait(timeout=deadline + 5)
        if result != 0:
            raise subprocess.CalledProcessError(result, args)
    finally:
        if process is not None:
            # Catchable cancellation cannot leave a launch client behind. The
            # unreaped direct child reserves this process-group identity.
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)


def private_directory(path):
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o077:
        raise ValueError('job directory is not private')


@contextlib.contextmanager
def locked_jobs():
    JOBS.mkdir(mode=0o700, exist_ok=True)
    private_directory(JOBS)
    descriptor = os.open(JOBS / 'runner.lock', os.O_RDWR | os.O_CREAT | os.O_CLOEXEC |
                         os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    with os.fdopen(descriptor, 'r+') as lock:
        info = os.fstat(lock.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                or info.st_mode & 0o022):
            raise ValueError('runner lock is not trusted')
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield lock


def systemctl(*args):
    return subprocess.run(['/usr/bin/systemctl', *args], env=ENV, stdin=subprocess.DEVNULL,
                          capture_output=True, text=True, timeout=10)


def service_state(unit):
    if UNIT.fullmatch(unit) is None:
        raise ValueError('unexpected media service name')
    names = ('LoadState', 'ActiveState', 'Transient', 'ControlGroup', 'MainPID', 'ControlPID',
             'KillMode', 'SendSIGKILL', 'FinalKillSignal', 'TimeoutStopUSec')
    result = systemctl('show', unit, '--property=' + ','.join(names))
    if result.returncode not in (0, 4):
        raise RuntimeError('service state unavailable')
    state = dict(line.split('=', 1) for line in result.stdout.splitlines())
    if set(state) != set(names):
        raise RuntimeError('incomplete service state')
    if state['LoadState'] == 'not-found':
        if not stopped(state):
            raise RuntimeError('missing service has unexpected active state')
    elif (state['LoadState'] != 'loaded' or state['Transient'] != 'yes'
          or state['KillMode'] != 'control-group' or state['SendSIGKILL'] != 'yes'
          or state['FinalKillSignal'] != '9' or state['TimeoutStopUSec'] != '2s'
          or state['ControlGroup'] not in ('', '/system.slice/' + unit)):
        raise ValueError('unrecognized media service policy')
    return state


def stopped(state):
    return (state['ActiveState'] in ('inactive', 'failed')
            and state['MainPID'] == '0' and state['ControlPID'] == '0')


def assert_no_processes(units, vmm_uid=None):
    for process in pathlib.Path('/proc').iterdir():
        if not process.name.isdecimal():
            continue
        try:
            membership = (process / 'cgroup').read_text()
            for line in membership.splitlines():
                path = line.split(':', 2)[2]
                if units.intersection(path.split('/')):
                    raise JobProcessesRemain('media service still has a process')
            if vmm_uid is not None:
                status = (process / 'status').read_text()
                uid = next(line for line in status.splitlines() if line.startswith('Uid:'))
                if str(vmm_uid) in uid.split()[1:]:
                    raise JobProcessesRemain('VMM identity still has a process')
        except (FileNotFoundError, ProcessLookupError):
            # A process that exited during inspection cannot retain job access.
            continue


def wait_for_no_processes(units, *, timeout=2):
    deadline = time.monotonic() + timeout
    while True:
        try:
            assert_no_processes(units)
            return
        except JobProcessesRemain:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise
            time.sleep(min(0.02, remaining))


def stop_job(unit):
    state = service_state(unit)
    if state['LoadState'] != 'not-found':
        result = systemctl('stop', unit)
        if result.returncode not in (0, 5):
            raise RuntimeError('could not stop media service')
    if not stopped(service_state(unit)):
        raise RuntimeError('service remains active; workspace retained')
    # Inactive unit metadata can precede disappearance of its final /proc entry.
    # Keep the same strict process check and retain storage if draining times out.
    wait_for_no_processes({unit})


def workspace_mounted(root):
    if root.parent != JOBS or WORKSPACE.fullmatch(root.name) is None:
        raise ValueError('unexpected workspace path')
    private_directory(root)
    target = str(root)
    found = []
    for line in pathlib.Path('/proc/self/mountinfo').read_text().splitlines():
        before, after = line.split(' - ', 1)
        fields = before.split()
        if fields[4].startswith(target + '/'):
            raise ValueError('nested mount requires operator inspection')
        if fields[4] == target:
            found.append((fields, after.split()))
    if found:
        if len(found) != 1:
            raise ValueError('ambiguous workspace mount')
        fields, filesystem = found[0]
        if fields[3] != '/' or filesystem[0] != 'tmpfs' or 'nosuid' not in fields[5].split(','):
            raise ValueError('unexpected workspace filesystem')
    elif next(root.iterdir(), None) is not None:
        raise ValueError('unmounted workspace is not empty')
    return bool(found)


def remove_workspace(root):
    if workspace_mounted(root):
        subprocess.run(['/usr/bin/umount', str(root)], env=ENV, check=True,
                       stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=10)
    # Never recursively delete, detach lazily, or follow a guest-selected path.
    private_directory(root)
    root.rmdir()


def reconcile_jobs():
    """Caller must hold locked_jobs(), also shared with any pending launch client."""
    roots = []
    units = set()
    for entry in JOBS.iterdir():
        if entry.name == 'runner.lock':
            continue
        if len(roots) >= MAX_ORPHANS:
            raise ValueError('too many abandoned jobs; operator inspection required')
        workspace_mounted(entry)
        unit = '26chan-media-' + WORKSPACE.fullmatch(entry.name).group(1) + '.service'
        if unit in units:
            raise ValueError('ambiguous workspaces for one service')
        units.add(unit)
        roots.append(entry)
    result = systemctl('list-units', '--all', '--no-legend', '--plain', '--full',
                       '26chan-media-*.service')
    if result.returncode != 0:
        raise RuntimeError('media service inventory unavailable')
    for line in result.stdout.splitlines():
        unit = line.split()[0]
        if UNIT.fullmatch(unit) is None:
            raise ValueError('unknown service in reserved media namespace')
        units.add(unit)
    if len(units) > MAX_ORPHANS:
        raise ValueError('too many abandoned services; operator inspection required')
    # Validate the entire inventory before changing a service or mount.
    for unit in sorted(units):
        service_state(unit)
    for unit in sorted(units):
        stop_job(unit)
    identity = pwd.getpwnam('board-media-vmm')
    if identity.pw_uid == 0 or identity.pw_gid == 0 or identity.pw_shell != '/usr/sbin/nologin':
        raise ValueError('VMM identity rejected')
    assert_no_processes(units, identity.pw_uid)
    for root in roots:
        remove_workspace(root)
