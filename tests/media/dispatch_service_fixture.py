"""Owned systemd lifecycle for the existing native dispatch qualification."""
import errno
import pathlib
import re
import socket
import subprocess

from test_dispatch import Exercise, HEX, REPO, SAFE, sql, wait_until


def systemctl(*args, required=True, timeout=15):
    result = subprocess.run(['/usr/bin/systemctl', *map(str, args)], env=SAFE,
                            stdin=subprocess.DEVNULL, capture_output=True, timeout=timeout)
    if required:
        assert result.returncode == 0, 'owned systemd operation failed: ' + str(args[0])
    return result


class UnitProcess:
    """Only the process operations used by Exercise, scoped to one owned unit."""
    def __init__(self, name):
        assert re.fullmatch(r'paperboard-dispatch-qualification-[a-z0-9_]{8}-(broker|gateway)\.service', name)
        self.name = name

    def state(self):
        result = systemctl('show', self.name, '--property=ActiveState,MainPID,ExecMainStatus,Result,InvocationID')
        return dict(line.split('=', 1) for line in result.stdout.decode().splitlines())

    @property
    def pid(self):
        pid = int(self.state()['MainPID'])
        assert pid > 0, 'owned unit has no live main process'
        return pid

    def poll(self):
        state = self.state()
        if state['ActiveState'] in ('active', 'activating', 'deactivating'):
            return None
        return int(state['ExecMainStatus'])

    def send_signal(self, signum):
        result = systemctl('kill', '--kill-who=main', '--signal=' + str(int(signum)), self.name,
                           required=False)
        assert result.returncode == 0 or self.poll() is not None, 'owned unit signal failed'

    def terminate(self):
        systemctl('stop', self.name, timeout=95)

    def communicate(self, timeout=15):
        wait_until(lambda: self.poll() is not None, timeout)
        return b'', b''


class SystemdExercise(Exercise):
    def __init__(self):
        super().__init__()
        token = self.root.name.removeprefix('26chan-dispatch-')
        self.broker_unit = UnitProcess(f'paperboard-dispatch-qualification-{token}-broker.service')
        self.gateway_unit = UnitProcess(f'paperboard-dispatch-qualification-{token}-gateway.service')
        self.unit_files = {}
        self.installed = False

    def environment(self, mode='development'):
        # Synthetic unrelated variables prove that the actual ExecStart clears them.
        self.write(self.root / 'dispatch.env',
                   f'APP_ENV={mode}\nGATEWAY_UID={self.gateway.pw_uid}\n'
                   'DATABASE_URL=synthetic-unused-credential\nQUALIFICATION_CANARY=must-not-survive\n')

    def render(self, kind, config):
        text = (REPO / f'deploy/media-dispatch-{kind}.service').read_text()
        substitutions = {
            '/etc/26chan-dispatch/dispatch.env': str(self.root / 'dispatch.env'),
        }
        if kind == 'broker':
            substitutions.update({
                'WorkingDirectory=/opt/paperboard/media': 'WorkingDirectory=' + str(self.media),
                '/opt/paperboard/media/dispatch-broker.py': str(self.media / 'dispatch-broker.py'),
                '/etc/26chan-dispatch/decode.json': config,
                '/run/26chan-media-dispatch': str(self.broker_dir),
            })
        else:
            # Two references preserve both BindsTo and ordering against our broker.
            assert text.count('media-dispatch-broker.service') == 2
            text = text.replace('media-dispatch-broker.service', self.broker_unit.name)
            substitutions.update({
                'WorkingDirectory=/opt/paperboard': 'WorkingDirectory=' + str(self.root),
                '/opt/paperboard/media-dispatch-gateway': str(self.bin / 'media-dispatch-gateway'),
                '/etc/26chan-dispatch/gateway.json': str(self.keys / 'gateway.json'),
                '-/etc/26chan-coordinator -/var/lib/26chan-coordinator':
                    '-' + str(self.private) + ' -' + str(self.hidden),
            })
        for before, after in substitutions.items():
            assert text.count(before) == 1, 'candidate path substitution changed'
            replacement = after.removeprefix('WorkingDirectory=')
            paths = replacement.split(' ') if before.startswith('-/') else [replacement]
            assert all(re.fullmatch(r'-?/[A-Za-z0-9_./-]+', path) for path in paths), 'unsupported fixture unit path'
            text = text.replace(before, after)
        return text.encode()

    def unit_file(self, unit, data):
        path = pathlib.Path('/run/systemd/system') / unit.name
        assert path.parent.resolve() == pathlib.Path('/run/systemd/system')
        if path in self.unit_files:
            assert path.read_bytes() == self.unit_files[path], 'owned unit file changed externally'
            path.write_bytes(data)
        else:
            with path.open('xb') as stream:
                stream.write(data)
            path.chmod(0o644)
        self.unit_files[path] = data

    def install(self):
        self.media = self.directory('media')
        for name in ('dispatch-broker.py', 'dispatch_protocol.py', 'run-job.py', 'job_lifecycle.py', 'verify-cgroups.py'):
            self.write(self.media / name, (REPO / 'scripts/media' / name).read_bytes())
        self.hidden = self.directory('hidden', mode=0o755)
        self.write(self.hidden / 'canary', b'public test witness', mode=0o644)
        self.policy = self.directory('policy', mode=0o755)
        self.write(self.policy / 'readable', b'public test witness', mode=0o644)
        self.write(self.policy / 'writable', b'public test witness', mode=0o666)
        self.environment()
        self.unit_file(self.broker_unit, self.render('broker', self.config))
        self.unit_file(self.gateway_unit, self.render('gateway', self.config))
        systemctl('daemon-reload')
        self.processes.extend((self.broker_unit, self.gateway_unit))
        self.installed = True

    def start_gateway(self):
        systemctl('start', self.gateway_unit.name)
        return self.gateway_unit

    def start_broker(self, config):
        if not self.installed:
            self.install()
        self.unit_file(self.broker_unit, self.render('broker', config))
        systemctl('daemon-reload')
        systemctl('reset-failed', self.broker_unit.name, self.gateway_unit.name, required=False)
        systemctl('start', self.broker_unit.name)

        def ready():
            assert self.broker_unit.poll() is None, 'candidate broker startup rejected'
            try:
                with socket.socket(socket.AF_UNIX) as sock:
                    sock.settimeout(.1)
                    sock.connect(str(self.broker_dir / 'broker.sock'))
                return True
            except OSError:
                return False

        wait_until(ready)
        if hasattr(self, 'gateway_process'):
            self.gateway_process = self.start_gateway()
            wait_until(self.listening)
        return self.broker_unit

    def stop(self, process):
        super().stop(process)
        if process is self.broker_unit and self.installed:
            wait_until(lambda: self.gateway_unit.poll() is not None)
            assert self.gateway_unit.state()['MainPID'] == '0', 'dependent gateway remains'

    def service_boundaries(self):
        self.lock_inode = (self.broker_dir / 'broker.lock').stat().st_ino
        for unit, uid, memory_limit, tasks in [
                (self.broker_unit, 0, 268435456, 64),
                (self.gateway_unit, self.gateway.pw_uid, 134217728, 32)]:
            process = pathlib.Path('/proc') / str(unit.pid)
            status = dict(line.split(':', 1) for line in (process / 'status').read_text().splitlines())
            assert set(status['Uid'].split()) == {str(uid)}
            environment = dict(item.split(b'=', 1) for item in (process / 'environ').read_bytes().split(b'\0') if item)
            assert set(environment) <= {b'PATH', b'APP_ENV', b'LC_CTYPE'}
            assert environment[b'APP_ENV'] == b'development'
            if uid:
                assert int(status['CapEff'], 16) == 0 and status['NoNewPrivs'].strip() == '1'
            self.kernel_limits(unit, process, memory_limit, tasks)

        def access(path, write=False, namespace=False):
            # Joining a mount namespace alone leaves the caller's root/cwd pinned
            # to its old mounts. Use the target root as well before dropping UID.
            prefix = ['/usr/bin/nsenter', '--target', str(self.gateway_unit.pid),
                      '--mount', '--root', '--wd=/', '--'] if namespace else []
            operation = 'p.write_bytes(b"owned write witness")' if write else 'p.read_bytes()'
            code = ('import pathlib,sys\np=pathlib.Path(sys.argv[1])\n'
                    f'try:\n {operation}\nexcept OSError as error:\n sys.exit(error.errno)\n')
            args = [*prefix, '/usr/bin/setpriv', f'--reuid={self.gateway.pw_uid}',
                    f'--regid={self.gateway.pw_gid}', '--clear-groups', '/usr/bin/python3', '-c', code, str(path)]
            return subprocess.run(args, env=SAFE, capture_output=True, timeout=5).returncode

        for namespace in (False, True):
            assert access(self.policy / 'readable', namespace=namespace) == 0, 'healthy namespace reader unavailable'
        assert access(self.hidden / 'canary') == 0
        assert access(self.hidden / 'canary', namespace=True) == errno.EACCES, 'InaccessiblePaths ineffective'
        assert access(self.policy / 'writable', write=True) == 0
        assert access(self.policy / 'writable', write=True, namespace=True) == errno.EROFS, 'runtime write restriction ineffective'
        assert access(self.policy / 'writable', write=True) == 0, 'allowed writer ceased working'
        print('PASS actual candidate identities, cleared environments, kernel limits and gateway mount restrictions', flush=True)

    def kernel_limits(self, unit, process, memory_limit, tasks):
        groups = {}
        for line in (process / 'cgroup').read_text().splitlines():
            _, controllers, path = line.split(':', 2)
            for controller in controllers.split(','):
                groups[controller] = path
        expected = '/system.slice/' + unit.name
        if 'memory' in groups:
            mounts = {}
            for line in pathlib.Path('/proc/mounts').read_text().splitlines():
                _, mount, kind, options, *_ = line.split()
                if kind == 'cgroup':
                    for controller in options.split(','):
                        mounts[controller] = pathlib.Path(mount)
            for controller in ('memory', 'cpu', 'pids'):
                assert groups[controller] == expected
            memory, cpu, pids = (mounts[name] / expected.lstrip('/') for name in ('memory', 'cpu', 'pids'))
            assert (memory / 'memory.limit_in_bytes').read_text().strip() == str(memory_limit)
            assert (cpu / 'cpu.cfs_quota_us').read_text() == (cpu / 'cpu.cfs_period_us').read_text()
        else:
            assert groups[''] == expected
            memory = cpu = pids = pathlib.Path('/sys/fs/cgroup') / expected.lstrip('/')
            assert (memory / 'memory.max').read_text().strip() == str(memory_limit)
            quota, period = (cpu / 'cpu.max').read_text().split()
            assert quota == period
        assert (pids / 'pids.max').read_text().strip() == str(tasks)

    def production_denial(self):
        self.stop(self.broker_unit)
        self.environment('production')
        try:
            self.rejected_start()
            self.clean_vm()
        finally:
            self.stop(self.gateway_unit)
            self.stop(self.broker_unit)
            self.environment()
        self.broker = self.start_broker(self.config)
        print('PASS production mode rejects candidate startup and broker stop drains its dependent gateway', flush=True)

    def rejected_start(self):
        previous = self.broker_unit.state()['InvocationID']
        systemctl('reset-failed', self.broker_unit.name, self.gateway_unit.name, required=False)
        systemctl('start', self.gateway_unit.name, required=False)
        wait_until(lambda: self.broker_unit.poll() is not None and self.gateway_unit.poll() is not None)
        state = self.broker_unit.state()
        assert state['InvocationID'] and state['InvocationID'] != previous, 'broker was not started'
        assert state['Result'] == 'exit-code' and state['ExecMainStatus'] == '1', 'expected broker rejection absent'
        assert state['MainPID'] == self.gateway_unit.state()['MainPID'] == '0'

    def retention_recovery(self):
        self.stop(self.broker_unit)
        witness = self.broker_dir / 'requests' / 'unknown-owned-witness'
        self.write(witness, b'retain this owned marker')
        try:
            self.unit_file(self.broker_unit, self.render('broker', self.config))
            systemctl('daemon-reload')
            self.rejected_start()
            assert witness.read_bytes() == b'retain this owned marker'
            assert (self.broker_dir / 'broker.lock').stat().st_ino == self.lock_inode
        finally:
            self.stop(self.gateway_unit)
            self.stop(self.broker_unit)
            assert witness.read_bytes() == b'retain this owned marker'
            witness.unlink()
        self.broker = self.start_broker(self.config)
        job = self.intake()
        asset = self.finish(self.dispatch()).decode().strip()
        assert HEX.fullmatch(asset)
        assert sql(f"SELECT count(*) FROM media.assets WHERE id='{asset}' AND job_id='{job}' AND state='approved'") == '1'
        self.clean_vm()
        assert (self.broker_dir / 'broker.lock').stat().st_ino == self.lock_inode
        print('PASS failed broker startup retains unknown state and inspected recovery restores actual processing', flush=True)

    def cleanup(self):
        # If stop/VM cleanup is uncertain, retain definitions and storage for inspection.
        if self.installed:
            self.stop(self.gateway_unit)
            self.stop(self.broker_unit)
        super().cleanup()
        for path, content in self.unit_files.items():
            assert path.read_bytes() == content, 'owned unit definition changed externally'
            path.unlink()
        if self.unit_files:
            systemctl('daemon-reload')
            systemctl('reset-failed', self.gateway_unit.name, self.broker_unit.name, required=False)
