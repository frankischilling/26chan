"""Owned operator update and distinct observer identities on disposable Linux."""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
import urllib.request

ENVIRONMENT = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'LANG': 'C', 'SYSTEMD_COLORS': '0'}


def run(command, *, allowed=(0,), timeout=20):
    try:
        result = subprocess.run([str(part) for part in command], stdin=subprocess.DEVNULL,
                                capture_output=True, env=ENVIRONMENT, timeout=timeout)
    except (OSError, subprocess.TimeoutExpired):
        raise AssertionError('Owned maintenance operation did not complete') from None
    if result.returncode not in allowed:
        raise AssertionError('Owned maintenance operation failed')
    return result.stdout.decode('utf-8').strip()


def private_json(path, value):
    temporary = path.with_suffix('.replacement')
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(descriptor, 'w') as output:
        json.dump(value, output)
    temporary.replace(path)


def wait_for(predicate, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError('Owned maintenance state did not arrive')


class OwnedFixture:
    def __init__(self, root):
        root = Path(root)
        if (not root.is_absolute() or not re.fullmatch(r'board-maintenance-monitor-[0-9a-f]{16}', root.name)
                or not root.is_dir() or root.is_symlink() or root.resolve() != root or root.parent != Path('/var/lib')):
            raise ValueError('Invalid owned maintenance root')
        self.root, self.identity = root, root.stat()
        self.tag = root.name.removeprefix('board-maintenance-monitor-')
        self.description = 'Owned board maintenance ' + self.tag
        self.observer_unit = 'board-maintenance-observer-' + self.tag + '.service'
        self.producer_unit = 'board-maintenance-producer-' + self.tag + '.service'
        self.public, self.private, self.states = (root / name for name in ('public', 'private', 'states'))

    def check_root(self):
        current = self.root.lstat()
        if (self.root.is_symlink() or self.root.resolve() != self.root
                or (current.st_dev, current.st_ino) != (self.identity.st_dev, self.identity.st_ino)):
            raise AssertionError('Owned maintenance root changed')

    def unit_info(self, unit):
        if unit not in (self.observer_unit, self.producer_unit):
            raise AssertionError('Unknown maintenance unit')
        text = run(['systemctl', 'show', unit,
                    '--property=LoadState,Description,ActiveState,ControlGroup,Result,ExecMainStatus'], allowed=(0, 1))
        return dict(line.split('=', 1) for line in text.splitlines() if '=' in line)

    def stop_unit(self, unit, *, require_success=False):
        self.check_root()
        state = self.unit_info(unit)
        if state.get('LoadState') == 'not-found':
            return
        if state.get('Description') != self.description:
            raise AssertionError('Refusing unrelated maintenance service')
        group = state.get('ControlGroup', '')
        if group and group != '/system.slice/' + unit:
            raise AssertionError('Maintenance cgroup identity changed')
        run(['systemctl', 'stop', unit])
        stopped = self.unit_info(unit)
        if require_success and stopped.get('LoadState') != 'not-found':
            if stopped.get('Result') not in (None, 'success') or stopped.get('ExecMainStatus') not in (None, '0'):
                raise AssertionError('Maintenance observer did not stop successfully')
        if group:
            path = Path('/sys/fs/cgroup') / group.lstrip('/')
            wait_for(lambda: not path.exists() or not (path / 'cgroup.procs').read_text().strip(), 10)
        run(['systemctl', 'reset-failed', unit], allowed=(0, 1))

    def cleanup(self):
        self.check_root()
        self.stop_unit(self.observer_unit)
        self.stop_unit(self.producer_unit)
        for path in (self.public, self.private, self.states):
            if path.exists():
                if path.is_symlink() or path.resolve().parent != self.root:
                    raise AssertionError('Maintenance cleanup identity changed')
                shutil.rmtree(path)

    def setup(self, binary):
        if os.geteuid() != 0 or not Path('/sys/fs/cgroup/cgroup.controllers').is_file():
            raise AssertionError('Owned Linux root and cgroup v2 are required')
        for unit in (self.observer_unit, self.producer_unit):
            if self.unit_info(unit).get('LoadState') != 'not-found':
                raise AssertionError('Maintenance unit name already exists')
        self.root.chmod(0o711)
        for path, mode in ((self.public, 0o755), (self.private, 0o700), (self.states, 0o755)):
            path.mkdir(mode=mode)
        shutil.copyfile(binary, self.public / 'board-maintenance-monitor')
        (self.public / 'board-maintenance-monitor').chmod(0o755)
        shutil.copyfile(Path(__file__), self.public / 'fixture.py')
        (self.public / 'fixture.py').chmod(0o644)
        self.config = self.public / 'observer.json'
        self.config.write_text(json.dumps({'targets': [{'target': 'application',
                              'path': str(self.states / 'application.json'),
                              'max_age_seconds': 30, 'run_timeout_seconds': 10}]}))
        self.config.chmod(0o644)
        self.update = self.private / 'update.py'
        self.update.write_text('#!/usr/bin/python3\nfrom pathlib import Path\nimport sys\n'
                               'source, destination = map(Path, sys.argv[1:])\n'
                               'data = source.read_bytes()\n'
                               'assert data in (b"version1", b"version2", b"version3")\n'
                               'temporary = destination.with_suffix(".new")\n'
                               'temporary.write_bytes(data)\n'
                               'temporary.replace(destination)\n'
                               'assert destination.read_bytes() == data\n')
        self.update.chmod(0o500)
        self.source, self.installed = self.private / 'source', self.private / 'installed'
        self.source.write_bytes(b'version1')
        # This actual root operation is a positive control for execute/read/write
        # denials below. The producer will independently perform its own update.
        run([self.update, self.source, self.installed])
        self.producer_config = self.private / 'producer.json'
        private_json(self.producer_config, {'target': 'application', 'state_directory': str(self.states),
                     'command': [str(self.update), str(self.source), str(self.installed)], 'timeout_seconds': 10})
        self.producer_script = Path(__file__).resolve().parents[2] / 'scripts/maintenance/run.py'
        self.produce()

    def produce(self, *, failure=False):
        self.check_root()
        self.stop_unit(self.producer_unit)
        command = ['systemd-run', '--quiet', '--wait', '--pipe', '--collect',
                   '--unit=' + self.producer_unit, '--description=' + self.description,
                   '--property=MemoryMax=128M', '--property=TasksMax=16', '--property=CPUQuota=50%',
                   '--property=RuntimeMaxSec=20', '--property=TimeoutStopSec=5',
                   '--property=KillMode=control-group', '--property=UMask=0077', '--',
                   '/usr/bin/env', '-i', 'PATH=' + ENVIRONMENT['PATH'], 'LANG=C',
                   '/usr/bin/python3', '-I', self.producer_script, self.producer_config]
        run(command, allowed=(1,) if failure else (0,), timeout=25)
        data = json.loads((self.states / 'application.json').read_text())
        if data['outcome'] != ('failure' if failure else 'success') or data['failure_pending'] != failure:
            raise AssertionError('Real producer outcome is not reflected in its journal')
        if not failure and self.installed.read_bytes() != self.source.read_bytes():
            raise AssertionError('Real update did not install its expected marker')

    def start_observer(self, address, token):
        base = dict(ENVIRONMENT, APP_ENV='production', MAINTENANCE_CONFIG_FILE=str(self.config),
                    METRICS_BIND_ADDR=address, METRICS_TOKEN=token)
        rejected = ['DATABASE_URL', 'TEST_PUBLIC_DATABASE_URL', 'MIGRATION_DATABASE_URL',
                    'MEDIA_DATABASE_URL', 'MEDIA_READ_DATABASE_URL', 'AUTH_DATABASE_URL',
                    'STAFF_DATABASE_URL', 'MONITOR_DATABASE_URL', 'PGPASSWORD']
        trials = [dict(base, **{key: ''}) for key in rejected]
        trials += [dict(base, APP_ENV=''), dict(base, APP_ENV='other')]
        trials += [{key: value for key, value in base.items() if key != 'APP_ENV'}]
        for environment in trials:
            result = subprocess.run([self.public / 'board-maintenance-monitor'], env=environment,
                                    stdin=subprocess.DEVNULL, capture_output=True, timeout=5)
            if result.returncode == 0 or token.encode() in result.stdout + result.stderr:
                raise AssertionError('Observer accepted unrelated credentials or exposed them')
        print('PASS actual observer startup rejects unrelated credentials and invalid environment', flush=True)
        env_path = self.private / 'observer.env'
        env_path.write_text('APP_ENV=production\nMAINTENANCE_CONFIG_FILE=' + str(self.config)
                            + '\nMETRICS_BIND_ADDR=' + address + '\nMETRICS_TOKEN=' + token + '\n')
        env_path.chmod(0o600)
        properties = ['DynamicUser=yes', 'PrivateTmp=yes', 'NoNewPrivileges=yes',
                      'ProtectSystem=strict', 'ProtectHome=yes', 'CapabilityBoundingSet=',
                      'ProtectControlGroups=yes', 'RestrictAddressFamilies=AF_INET AF_INET6',
                      'MemoryMax=128M', 'TasksMax=16', 'CPUQuota=25%', 'RuntimeMaxSec=480',
                      'TimeoutStopSec=5', 'KillMode=control-group', 'Restart=no',
                      'EnvironmentFile=' + str(env_path),
                      'StandardOutput=append:' + str(self.private / 'observer.log'), 'StandardError=inherit',
                      'ExecStartPost=/usr/bin/python3 ' + str(self.public / 'fixture.py') + ' verify ' + str(self.root)]
        run(['systemd-run', '--quiet', '--unit=' + self.observer_unit, '--description=' + self.description,
             *['--property=' + value for value in properties], '--', '/usr/bin/python3',
             self.public / 'fixture.py', 'observe', self.root])


def probe(root):
    if os.geteuid() == 0:
        raise AssertionError('Observer identity is not distinct')
    journal = root / 'states/application.json'
    if json.loads(journal.read_text())['outcome'] != 'success':
        raise AssertionError('Observer cannot read the genuine healthy journal')
    for path in (journal, root / 'public/observer.json'):
        try:
            with path.open('ab'):
                pass
        except OSError as error:
            if error.errno not in (1, 13, 30):
                raise
        else:
            raise AssertionError('Observer can modify maintenance authority or state')
    try:
        (root / 'private/source').read_bytes()
    except PermissionError:
        pass
    else:
        raise AssertionError('Observer can read operator payload')
    try:
        subprocess.run([root / 'private/update.py'], capture_output=True, timeout=2, env=ENVIRONMENT)
    except PermissionError:
        pass
    else:
        raise AssertionError('Observer can execute operator update payload')
    print('PASS distinct observer reads journal and denies state/config writes and operator payload access', flush=True)


def verify(root):
    url = 'http://' + os.environ['METRICS_BIND_ADDR'] + '/readyz'
    client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        try:
            request = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + os.environ['METRICS_TOKEN']})
            with client.open(request, timeout=2) as response:
                if response.status == 200:
                    probe(root)
                    return
        except OSError:
            pass
        time.sleep(0.1)
    raise AssertionError('Observer authority probe did not reach authenticated readiness')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('observe', 'verify'))
    parser.add_argument('root', type=Path)
    args = parser.parse_args()
    if args.action == 'verify':
        verify(args.root)
    else:
        probe(args.root)
        binary = args.root / 'public/board-maintenance-monitor'
        os.execve(binary, [str(binary)], os.environ)
