"""Finite Linux-only pressure in an exclusively owned mount and service cgroup."""

import argparse
import gc
import json
import mmap
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.request


ENVIRONMENT = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C', 'SYSTEMD_COLORS': '0'}
MEMORY_LIMIT = 64 * 1024 * 1024
TASK_LIMIT = 16
STORAGE_LIMIT = 16 * 1024 * 1024
INODE_LIMIT = 256


def run(command, *, timeout=20, allowed=(0,)):
    name = Path(str(command[0])).name
    operation = {'systemctl': 'inspect or stop owned service', 'systemd-run': 'start owned service',
                 'mount': 'mount or remount owned tmpfs', 'umount': 'unmount owned tmpfs',
                 'findmnt': 'verify owned mount identity'}.get(name, 'owned native command')
    try:
        result = subprocess.run([str(part) for part in command], stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                env=ENVIRONMENT, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        print('STAGE failed operation: ' + operation, flush=True)
        raise AssertionError('Owned resource fixture operation failed') from None
    if result.returncode not in allowed:
        print('STAGE failed operation: ' + operation + ' (exit ' + str(result.returncode) + ')', flush=True)
        raise AssertionError('Owned resource fixture operation failed')
    return result.stdout.decode('utf-8', errors='strict').strip()


def private_json(path, value):
    temporary = path.with_suffix('.replacement')
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w', encoding='utf-8') as output:
        json.dump(value, output)
    temporary.replace(path)


def wait_for(predicate, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError('Owned resource fixture did not reach its required state')


class OwnedFixture:
    def __init__(self, root):
        root = Path(root)
        if (not root.is_absolute() or not re.fullmatch(r'board-resource-monitor-[0-9a-f]{16}', root.name)
                or not root.is_dir() or root.is_symlink() or root.resolve() != root):
            raise ValueError('Invalid owned resource root')
        self.root = root
        self.identity = root.stat()
        self.tag = root.name.removeprefix('board-resource-monitor-')
        self.description = 'Owned board resource ' + self.tag
        self.fixture_unit = 'board-resource-fixture-' + self.tag + '.service'
        self.observer_unit = 'board-resource-observer-' + self.tag + '.service'
        self.mount = root / 'storage'
        self.source = self.mount / 'observed'
        self.public = root / 'public'
        self.private = root / 'private'
        self.control = self.private / 'fixture.sock'
        self.cgroup = Path('/sys/fs/cgroup/system.slice') / self.fixture_unit

    def check_root(self):
        current = self.root.lstat()
        if (self.root.is_symlink() or self.root.resolve() != self.root
                or (current.st_dev, current.st_ino) != (self.identity.st_dev, self.identity.st_ino)):
            raise AssertionError('Owned resource root identity changed')

    def unit_info(self, unit):
        output = run(['systemctl', 'show', unit, '--property=LoadState,Description,ActiveState,ControlGroup,Result,ExecMainStatus'], allowed=(0, 1))
        return dict(line.split('=', 1) for line in output.splitlines() if '=' in line)

    def stop_unit(self, unit, *, require_success=False):
        self.check_root()
        state = self.unit_info(unit)
        if state.get('LoadState') == 'not-found':
            return
        if state.get('Description') != self.description:
            raise AssertionError('Refusing to stop an unowned resource service')
        relative = state.get('ControlGroup', '')
        if relative and relative != '/system.slice/' + unit:
            raise AssertionError('Owned service cgroup identity changed')
        run(['systemctl', 'stop', unit])
        stopped = self.unit_info(unit)
        if require_success and stopped.get('LoadState') != 'not-found':
            if (stopped.get('ActiveState') == 'failed' or stopped.get('Result') not in (None, 'success')
                    or stopped.get('ExecMainStatus') not in (None, '0')):
                raise AssertionError('Owned resource observer did not exit successfully on normal stop')
        path = Path('/sys/fs/cgroup') / relative.lstrip('/') if relative else None
        if path is not None:
            wait_for(lambda: not path.exists() or not (path / 'cgroup.procs').read_text().strip(), 10)
        run(['systemctl', 'reset-failed', unit], allowed=(0, 1))

    def mount_info(self):
        if not self.mount.exists():
            return None
        output = run(['findmnt', '--mountpoint', self.mount, '--json', '--output', 'TARGET,SOURCE,FSTYPE'], allowed=(0, 1))
        if not output:
            return None
        entries = json.loads(output).get('filesystems', [])
        if len(entries) != 1 or entries[0] != {'target': str(self.mount), 'source': 'board-resource-' + self.tag, 'fstype': 'tmpfs'}:
            raise AssertionError('Refusing to change an unowned resource mount')
        return entries[0]

    def cleanup(self):
        self.check_root()
        # Services are tracked by exclusive transient names AND descriptions;
        # no process names, guessed PIDs or unrelated service units are stopped.
        self.stop_unit(self.observer_unit)
        self.stop_unit(self.fixture_unit)
        if self.mount_info():
            run(['umount', self.mount])
        if self.mount_info():
            raise AssertionError('Owned resource mount survived cleanup')
        for directory in (self.public, self.private, self.mount):
            if directory.exists():
                if directory.is_symlink() or directory.resolve().parent != self.root:
                    raise AssertionError('Owned cleanup child identity changed')
                shutil.rmtree(directory)

    def setup(self, binary):
        print('STAGE verify native cgroup and exclusive service prerequisites', flush=True)
        if not sys.platform.startswith('linux') or os.geteuid() != 0:
            raise AssertionError('Native resource qualification requires an owned Linux root session')
        self.check_root()
        if not Path('/sys/fs/cgroup/cgroup.controllers').is_file():
            raise AssertionError('Native resource qualification requires cgroup v2')
        for unit in (self.fixture_unit, self.observer_unit):
            if self.unit_info(unit).get('LoadState') != 'not-found':
                raise AssertionError('Exclusive resource service name is already in use')
        self.root.chmod(0o711)
        self.public.mkdir(mode=0o755)
        self.private.mkdir(mode=0o700)
        self.mount.mkdir(mode=0o711)
        shutil.copyfile(binary, self.public / 'board-resource-monitor')
        (self.public / 'board-resource-monitor').chmod(0o755)
        shutil.copyfile(Path(__file__), self.public / 'resource_fixture.py')
        (self.public / 'resource_fixture.py').chmod(0o644)
        print('STAGE create bounded owned tmpfs', flush=True)
        run(['mount', '-t', 'tmpfs', '-o', 'size=16m,nr_inodes=256,mode=0711,nodev,nosuid,noexec',
             'board-resource-' + self.tag, self.mount])
        self.mount_info()
        self.source.mkdir(mode=0o711)
        payload = self.source / 'protected-payload'
        descriptor = os.open(payload, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'wb') as output:
            output.write(b'owned harmless payload\n')
        # Root fixture control and a readable harmless payload are positive
        # controls for the observer's later permission denials.
        if payload.read_bytes() != b'owned harmless payload\n':
            raise AssertionError('Owned payload control was not readable')
        print('STAGE start finite resource pressure service', flush=True)
        self.start_unit(self.fixture_unit, ['/usr/bin/python3', self.public / 'resource_fixture.py',
                                           'serve', self.control, self.cgroup], dynamic=False)
        wait_for(self.control.exists)
        if self.command('ping') != {'ok': True}:
            raise AssertionError('Owned pressure fixture control is unavailable')
        if (int((self.cgroup / 'memory.max').read_text()) != MEMORY_LIMIT
                or int((self.cgroup / 'pids.max').read_text()) != TASK_LIMIT
                or (self.cgroup / 'memory.oom.group').read_text().strip() != '0'
                or (self.cgroup / 'cpu.max').read_text().split() != ['20000', '100000']):
            raise AssertionError('Native fixture ceilings differ from the required finite bounds')
        (self.cgroup / 'memory.max').write_text(str(MEMORY_LIMIT))
        self.config = self.public / 'resource.json'
        self.config.write_text(json.dumps({'storages': [{'target': 'database', 'path': str(self.source)}],
                                         'services': [{'target': 'public', 'path': str(self.cgroup)}]}))
        self.config.chmod(0o644)

    def start_unit(self, unit, command, *, dynamic, environment_file=None):
        properties = ['MemoryMax=64M', 'MemorySwapMax=0', 'TasksMax=16', 'CPUQuota=20%',
                      'CPUQuotaPeriodSec=100ms', 'RuntimeMaxSec=600', 'TimeoutStopSec=5',
                      'KillMode=control-group', 'NoNewPrivileges=yes', 'CapabilityBoundingSet=',
                      'ProtectControlGroups=yes', 'ProtectSystem=strict', 'ProtectHome=yes',
                      'RestrictSUIDSGID=yes', 'UMask=0077', 'Restart=no',
                      'StandardOutput=append:' + str(self.private / (unit + '.log')),
                      'StandardError=inherit']
        if dynamic:
            properties += ['DynamicUser=yes', 'PrivateTmp=yes', 'RestrictAddressFamilies=AF_INET AF_INET6',
                           # Preserve the actual host mount flags for statvfs.
                           # Root ownership/DAC denies writes by the DynamicUser.
                           'ReadWritePaths=' + str(self.mount), 'InaccessiblePaths=-/run/systemd/private -/run/dbus/system_bus_socket']
            properties += ['ExecStartPost=/usr/bin/python3 ' + str(self.public / 'resource_fixture.py')
                           + ' verify ' + str(self.source) + ' ' + str(self.cgroup)]
        else:
            properties += ['ReadWritePaths=' + str(self.private), 'OOMPolicy=continue']
        if environment_file:
            properties += ['EnvironmentFile=' + str(environment_file)]
        run(['systemd-run', '--quiet', '--unit=' + unit, '--description=' + self.description,
             *['--property=' + value for value in properties], '--', *command])

    def start_observer(self, address, token):
        environment = self.private / 'observer.env'
        descriptor = os.open(environment, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, 'w') as output:
            output.write('APP_ENV=development\nRESOURCE_CONFIG_FILE=' + str(self.config)
                         + '\nMETRICS_BIND_ADDR=' + address + '\nMETRICS_TOKEN=' + token + '\n')
        self.start_unit(self.observer_unit,
                        ['/usr/bin/python3', self.public / 'resource_fixture.py', 'observe',
                         self.source, self.cgroup, self.public / 'board-resource-monitor'],
                        dynamic=True, environment_file=environment)

    def command(self, command):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
            client.settimeout(10)
            client.connect(str(self.control))
            client.sendall((command + '\n').encode('ascii'))
            with client.makefile('rb') as source:
                response = source.readline(1025)
        if len(response) > 1024 or json.loads(response) != {'ok': True}:
            raise AssertionError('Owned pressure fixture rejected its bounded command')
        return {'ok': True}

    def bytes_pressure(self, enabled):
        self.mount_info()
        path = self.mount / 'bytes-pressure'
        if enabled:
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(descriptor, 'wb') as output:
                for _ in range(15):
                    output.write(b'x' * 1024 * 1024)
        else:
            path.unlink()

    def inode_pressure(self, enabled):
        self.mount_info()
        # 239 tiny files + mount root/payload cross 90% of exactly 256 inodes.
        for number in range(239):
            path = self.mount / ('inode-' + str(number))
            if enabled:
                descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                os.close(descriptor)
            else:
                path.unlink()

    def readonly(self, enabled):
        self.mount_info()
        run(['mount', '-o', 'remount,' + ('ro' if enabled else 'rw'), self.mount])
        if bool(os.statvfs(self.source).f_flag & os.ST_RDONLY) != enabled:
            raise AssertionError('Owned writer mount did not change its real read-only state')
        path = self.source / 'readonly-write-control'
        try:
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        except OSError as error:
            if not enabled or error.errno != 30:
                raise
        else:
            os.close(descriptor)
            path.unlink()
            if enabled:
                raise AssertionError('Read-only writer mount still accepted a payload write')


def probe_authority(mount, cgroup):
    if os.geteuid() == 0:
        raise AssertionError('Observer must have its distinct unprivileged identity')
    if not (cgroup / 'memory.current').read_text().strip().isdigit() or os.statvfs(mount).f_blocks <= 0:
        raise AssertionError('Observer statistics healthy control failed')
    try:
        (mount / 'protected-payload').read_bytes()
    except PermissionError:
        pass
    else:
        raise AssertionError('Observer could read protected payload data')
    try:
        descriptor = os.open(mount / 'forbidden-write', os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except PermissionError:
        pass
    else:
        os.close(descriptor)
        raise AssertionError('Observer could create a storage payload')
    try:
        descriptor = os.open(mount / 'protected-payload', os.O_WRONLY)
    except PermissionError:
        pass
    else:
        os.close(descriptor)
        raise AssertionError('Observer could modify protected payload data')
    try:
        descriptor = os.open(cgroup / 'memory.max', os.O_WRONLY)
    except OSError as error:
        if error.errno not in (13, 30):
            raise
    else:
        os.close(descriptor)
        raise AssertionError('Observer could open a cgroup control for writing')
    print('PASS distinct observer reads statistics and denies payload/control authority', flush=True)


def observe(mount, cgroup, binary):
    probe_authority(mount, cgroup)
    environment = {name: os.environ[name] for name in
                   ('APP_ENV', 'RESOURCE_CONFIG_FILE', 'METRICS_BIND_ADDR', 'METRICS_TOKEN')}
    os.execve(binary, [str(binary)], environment)


def verify(mount, cgroup):
    http = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def healthy():
        try:
            request = urllib.request.Request('http://' + os.environ['METRICS_BIND_ADDR'] + '/metrics',
                                             headers={'Authorization': 'Bearer ' + os.environ['METRICS_TOKEN']})
            with http.open(request, timeout=1) as response:
                return response.status == 200 and b'board_resource_sample_success 1\n' in response.read(65536)
        except OSError:
            return False

    wait_for(healthy, 15)
    probe_authority(mount, cgroup)


def serve(control, cgroup):
    if (int((cgroup / 'memory.max').read_text()) != MEMORY_LIMIT
            or int((cgroup / 'pids.max').read_text()) != TASK_LIMIT):
        raise AssertionError('Refusing workload outside its finite native cgroup')
    memory = []
    children = []

    def stop_children():
        for child in children:
            if child.poll() is None:
                child.terminate()
        for child in children:
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=3)
        children.clear()

    def stop_signal(signum, _frame):
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, stop_signal)
    deadline = time.monotonic() + 570
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            server.bind(str(control))
            control.chmod(0o600)
            server.listen(1)
            server.settimeout(1)
            while time.monotonic() < deadline:
                try:
                    peer, _address = server.accept()
                except socket.timeout:
                    continue
                with peer:
                    peer.settimeout(2)
                    with peer.makefile('rb') as source:
                        command = source.readline(32).decode('ascii').strip()
                    if command == 'ping':
                        pass
                    elif command == 'memory_on':
                        for _ in range(1024):
                            if int((cgroup / 'memory.current').read_text()) > MEMORY_LIMIT * 0.94:
                                break
                            block = mmap.mmap(-1, 65536)
                            block.write(b'x' * 65536)
                            memory.append(block)
                        else:
                            raise AssertionError('Bounded memory fixture did not reach pressure')
                    elif command == 'memory_off':
                        for block in memory:
                            block.close()
                        memory.clear()
                        gc.collect()
                    elif command == 'tasks_on':
                        for _ in range(TASK_LIMIT - 1):
                            if int((cgroup / 'pids.current').read_text()) > TASK_LIMIT * 0.9:
                                break
                            children.append(subprocess.Popen(['/usr/bin/sleep', '120'], env=ENVIRONMENT))
                    elif command == 'cpu_on':
                        children.append(subprocess.Popen(['/usr/bin/python3', str(Path(__file__)), 'cpu'], env=ENVIRONMENT))
                    elif command in ('tasks_off', 'cpu_off'):
                        stop_children()
                    elif command == 'oom':
                        child = subprocess.Popen(['/usr/bin/python3', str(Path(__file__)), 'oom'], env=ENVIRONMENT)
                        children.append(child)
                        if child.wait(timeout=10) != -signal.SIGKILL:
                            raise AssertionError('Bounded cgroup allocation did not produce a native OOM kill')
                        children.remove(child)
                    else:
                        raise AssertionError('Unknown owned fixture command')
                    peer.sendall(b'{"ok":true}\n')
    finally:
        stop_children()
        control.unlink(missing_ok=True)


def worker(kind):
    membership = Path('/proc/self/cgroup').read_text().strip()
    match = re.fullmatch(r'0::(/system.slice/board-resource-fixture-[0-9a-f]{16}\.service)', membership)
    if not match:
        raise AssertionError('Refusing pressure outside the owned fixture cgroup')
    cgroup = Path('/sys/fs/cgroup') / match[1].lstrip('/')
    if (int((cgroup / 'memory.max').read_text()) != MEMORY_LIMIT
            or int((cgroup / 'pids.max').read_text()) != TASK_LIMIT):
        raise AssertionError('Refusing pressure without finite owned ceilings')
    if kind == 'cpu':
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            sum(range(10000))
    else:
        # This child belongs to the already verified 64 MiB cgroup. Prefer it
        # over the finite fixture supervisor if the local OOM killer must choose.
        Path('/proc/self/oom_score_adj').write_text('1000')
        blocks = []
        for _ in range(80):
            blocks.append(bytearray(b'x' * 1024 * 1024))
        raise AssertionError('Native finite cgroup unexpectedly admitted 80 MiB')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('serve', 'observe', 'verify', 'cpu', 'oom'))
    parser.add_argument('paths', nargs='*', type=Path)
    arguments = parser.parse_args()
    if arguments.action == 'serve':
        serve(*arguments.paths)
    elif arguments.action == 'observe':
        observe(*arguments.paths)
    elif arguments.action == 'verify':
        verify(*arguments.paths)
    else:
        worker(arguments.action)
