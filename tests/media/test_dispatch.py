#!/usr/bin/env python3
"""Actual queue/TLS/nonroot gateway/root broker/Firecracker/approval qualification.

Root tests on an idle owned disposable queue only. Service processes receive
individual credentials in cleared environments. No credential value is logged.
"""
import hashlib
import json
import os
import pathlib
import pwd
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.parse

from test_vm import REPO, VmTest, red_png

SAFE = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'APP_ENV': 'development'}
HEX = re.compile('[0-9a-f]{32}')
PG = '/usr/lib/postgresql/16/bin/psql'


def sql(statement):
    # Credentials travel via process environment, never argv or diagnostics.
    url = urllib.parse.urlparse(os.environ['MIGRATION_DATABASE_URL'])
    pg_env = dict(PGHOST=url.hostname, PGPORT=str(url.port or 5432),
                  PGUSER=urllib.parse.unquote(url.username), PGPASSWORD=urllib.parse.unquote(url.password),
                  PGDATABASE=urllib.parse.unquote(url.path.lstrip('/')))
    result = subprocess.run([PG, '-XAt', '-v', 'ON_ERROR_STOP=1'],
                            input=statement, text=True, capture_output=True, timeout=10,
                            env={**SAFE, **pg_env})
    if result.returncode:
        raise AssertionError('owned database assertion failed')
    return result.stdout.strip()


def account(name):
    try:
        user = pwd.getpwnam(name)
    except KeyError:
        subprocess.run(['useradd', '--system', '--no-create-home', '--shell',
                        '/usr/sbin/nologin', name], check=True, capture_output=True)
        user = pwd.getpwnam(name)
    assert user.pw_uid != 0 and user.pw_gid != 0 and user.pw_shell == '/usr/sbin/nologin'
    return user


def wait_until(predicate, seconds=8):
    end = time.monotonic() + seconds
    while not predicate():
        assert time.monotonic() < end, 'owned synchronization deadline exceeded'
        time.sleep(.02)


class Exercise:
    def __init__(self):
        assert os.geteuid() == 0, 'explicit native root qualification required'
        self.config = os.environ['MEDIA_VM_TEST_CONFIG']
        self.probe = os.environ['MEDIA_VM_PROBE_CONFIG']
        self.binaries = pathlib.Path(os.environ.get('MEDIA_DISPATCH_BIN_DIR', REPO / 'target/debug')).resolve()
        for name in ('board-media-admin', 'media-publish', 'media-read', 'media-dispatch-gateway'):
            assert (self.binaries / name).is_file(), 'required native Rust binary missing'
        # Verify both supplied configurations with the real loader before intake.
        sys.path.insert(0, str(REPO / 'scripts/media'))
        import importlib.util
        spec = importlib.util.spec_from_file_location('dispatch_runner', REPO / 'scripts/media/run-job.py')
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        runner.configuration(self.config)
        runner.configuration(self.probe)
        assert sql("SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing')") == '0', 'use an idle disposable queue'
        self.gateway = account('board-media-gateway')
        self.coordinator = account('board-media-coordinator')
        self.vmm = pwd.getpwnam('board-media-vmm')
        assert len({self.gateway.pw_uid, self.coordinator.pw_uid, self.vmm.pw_uid, 0}) == 4
        self.root = pathlib.Path(tempfile.mkdtemp(prefix='26chan-dispatch-', dir='/run'))
        # Publication fsync walks ancestors; private children remain mode0700.
        self.root.chmod(0o755)
        self.broker_dir = self.root / 'broker'
        self.ids = []
        self.processes = []

    def directory(self, name, user=None, mode=0o700):
        path = self.root / name
        path.mkdir(mode=mode)
        path.chmod(mode)
        if user:
            os.chown(path, user.pw_uid, user.pw_gid)
        return path

    def write(self, path, data, user=None, mode=0o600):
        path.write_bytes(data.encode() if isinstance(data, str) else data)
        path.chmod(mode)
        if user:
            os.chown(path, user.pw_uid, user.pw_gid)

    def launch(self, args, user=None, env=None):
        options = dict(env=env or SAFE, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE, start_new_session=True)
        if user:
            options.update(user=user.pw_uid, group=user.pw_gid, extra_groups=[])
        process = subprocess.Popen([str(arg) for arg in args], **options)
        self.processes.append(process)
        return process

    def finish(self, process, success=True):
        out, err = process.communicate(timeout=35)
        assert (process.returncode == 0) == success, 'owned service command outcome differs'
        if not success:
            assert out == b'', 'failed dispatch must emit no object ID'
        return out

    def stop(self, process):
        if process.poll() is None:
            process.send_signal(signal.SIGCONT)
            process.terminate()
        process.communicate(timeout=15)

    def setup(self):
        self.bin = self.directory('bin', mode=0o755)
        for name in ('board-media-admin', 'media-publish', 'media-read', 'media-dispatch-gateway'):
            shutil.copyfile(self.binaries / name, self.bin / name)
            (self.bin / name).chmod(0o755)
        self.private = self.directory('coordinator', self.coordinator)
        self.keys = self.directory('gateway', mode=0o755)
        pki = self.directory('pki')

        def openssl(*args):
            result = subprocess.run(['openssl', *map(str, args)], capture_output=True, timeout=15, env=SAFE)
            assert result.returncode == 0, 'synthetic PKI generation failed'
            return result.stdout

        openssl('req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', pki / 'ca.key',
                '-out', pki / 'ca.pem', '-subj', '/CN=Owned dispatch qualification CA', '-days', '2')
        for name, usage in [('server', 'serverAuth'), ('client', 'clientAuth')]:
            openssl('req', '-newkey', 'rsa:2048', '-nodes', '-keyout', pki / f'{name}.key',
                    '-out', pki / f'{name}.csr', '-subj', '/CN=dispatch.test')
            self.write(pki / 'extensions', f'subjectAltName=DNS:dispatch.test\nextendedKeyUsage={usage}\nbasicConstraints=CA:FALSE\n')
            openssl('x509', '-req', '-in', pki / f'{name}.csr', '-CA', pki / 'ca.pem',
                    '-CAkey', pki / 'ca.key', '-CAcreateserial', '-out', pki / f'{name}.pem',
                    '-days', '2', '-extfile', pki / 'extensions')
        self.fingerprint = hashlib.sha256(openssl('x509', '-in', pki / 'client.pem', '-outform', 'DER')).hexdigest()
        for destination, user, name in [(self.keys, self.gateway, 'server'), (self.private, self.coordinator, 'client')]:
            for suffix in ('key', 'pem'):
                self.write(destination / f'{name}.{suffix}', (pki / f'{name}.{suffix}').read_bytes(), user)
            self.write(destination / 'ca.pem', (pki / 'ca.pem').read_bytes(), user)
        self.write(self.keys / 'authorized', self.fingerprint + '\n', mode=0o644)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            self.port = sock.getsockname()[1]
        self.client_config = dict(endpoint=f'127.0.0.1:{self.port}', server_name='dispatch.test',
                                  server_ca=str(self.private / 'ca.pem'), client_certificate=str(self.private / 'client.pem'),
                                  client_key=str(self.private / 'client.key'))
        self.write(self.private / 'client.json', json.dumps(self.client_config), self.coordinator)
        gateway_config = dict(listen=f'127.0.0.1:{self.port}', server_certificate=str(self.keys / 'server.pem'),
                              server_key=str(self.keys / 'server.key'), client_ca=str(self.keys / 'ca.pem'),
                              authorization_file=str(self.keys / 'authorized'), broker_socket=str(self.broker_dir / 'broker.sock'))
        self.write(self.keys / 'gateway.json', json.dumps(gateway_config), mode=0o644)
        # Private witness files contain the existing credentials, never rotated.
        self.write(self.private / 'writer.credential', os.environ['MEDIA_DATABASE_URL'], self.coordinator)
        self.write(self.private / 'reader.credential', os.environ['MEDIA_READ_DATABASE_URL'], self.coordinator)
        self.writer = {**SAFE, 'MEDIA_DATABASE_URL': os.environ['MEDIA_DATABASE_URL'],
                       'MEDIA_QUARANTINE_DIR': str(self.private / 'quarantine')}
        self.reader = {**SAFE, 'MEDIA_READ_DATABASE_URL': os.environ['MEDIA_READ_DATABASE_URL']}
        self.broker = self.start_broker(self.config)
        self.gateway_process = self.launch([self.bin / 'media-dispatch-gateway', self.keys / 'gateway.json'], self.gateway)
        wait_until(self.listening)

    def listening(self):
        assert self.gateway_process.poll() is None, 'native gateway startup rejected'
        try:
            with socket.create_connection(('127.0.0.1', self.port), timeout=.1):
                return True
        except OSError:
            return False

    def start_broker(self, config):
        process = self.launch(['python3', REPO / 'scripts/media/dispatch-broker.py', config,
                               self.broker_dir, self.gateway.pw_uid])
        def ready():
            assert process.poll() is None, 'native broker startup rejected'
            return (self.broker_dir / 'broker.sock').exists()
        wait_until(ready)
        return process

    def intake(self, payload=red_png()):
        assert sql("SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing')") == '0', 'queue became busy'
        source = self.private / 'input.png'
        self.write(source, payload, self.coordinator)
        result = self.finish(self.launch([self.bin / 'board-media-admin', 'intake', source, 'untrusted/display.png'], self.coordinator, self.writer))
        job = json.loads(result)['id']
        assert HEX.fullmatch(job)
        self.ids.append(job)
        return job

    def dispatch(self):
        return self.launch([self.bin / 'media-publish', 'dispatch', self.private / 'client.json',
                            self.private / 'objects'], self.coordinator, self.writer)

    def clean_vm(self):
        VmTest().assert_clean()
        assert list((self.broker_dir / 'requests').iterdir()) == [], 'broker staging remains'

    def no_approval(self, job):
        assert HEX.fullmatch(job)
        assert sql(f"SELECT count(*) FROM media.assets WHERE job_id='{job}'") == '0'

    def retire(self, job):
        assert HEX.fullmatch(job)
        sql(f"UPDATE media.jobs SET state='failed',lease_token=NULL,expires_at=NULL,failure='abandoned' WHERE id='{job}' AND state <> 'published'")

    def read(self, identity, path, allowed):
        result = subprocess.run(['python3', '-c', 'import pathlib,sys; pathlib.Path(sys.argv[1]).read_bytes()', str(path)],
                                user=identity.pw_uid, group=identity.pw_gid, extra_groups=[], env=SAFE,
                                capture_output=True, timeout=3)
        assert (result.returncode == 0) == allowed, 'identity file permission outcome differs'

    def exercise(self):
        job = self.intake()
        asset = self.finish(self.dispatch()).decode().strip()
        assert HEX.fullmatch(asset) and asset != job
        assert sql(f"SELECT count(*) FROM media.assets WHERE id='{asset}' AND job_id='{job}' AND state='approved' AND width=1 AND height=1") == '1'
        destination = self.private / 'read.png'
        self.finish(self.launch([self.bin / 'media-read', asset, self.private / 'objects', destination], self.coordinator, self.reader))
        assert destination.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        assert destination.read_bytes() == (self.private / 'objects' / f'{asset}.png').read_bytes()
        denied = self.private / 'denied.png'
        self.finish(self.launch([self.bin / 'media-read', job, self.private / 'objects', denied], self.coordinator, self.reader), False)
        assert not denied.exists()
        sql(f"DELETE FROM media.jobs WHERE id='{job}' AND state='published'")
        self.finish(self.launch([self.bin / 'media-read', asset, self.private / 'objects', self.private / 'after-cleanup.png'], self.coordinator, self.reader))
        self.clean_vm()
        print('PASS actual queue -> Rust mTLS -> nonroot gateway -> root broker -> Firecracker -> approval -> restricted reader', flush=True)

        for identity, allowed_path in [(self.gateway, self.keys / 'server.key'), (self.coordinator, self.private / 'client.key')]:
            self.read(identity, allowed_path, True)
        for path in [self.private / 'client.key', self.private / 'writer.credential', self.private / 'reader.credential',
                     self.private / 'quarantine' / f'{job}.input', self.private / 'objects' / f'{asset}.png']:
            self.read(self.gateway, path, False)
            self.read(self.vmm, path, False)
        self.read(self.coordinator, self.keys / 'server.key', False)
        self.read(self.vmm, self.keys / 'server.key', False)
        witness = self.broker_dir / 'requests' / 'permission-witness'
        self.write(witness, b'private root staging')
        try:
            assert witness.read_bytes() == b'private root staging'
            for identity in [self.gateway, self.coordinator, self.vmm]:
                self.read(identity, witness, False)
        finally:
            witness.unlink()
        print('PASS actual gateway/coordinator/VMM identity credential and storage denials with healthy allowed readers', flush=True)

        # A trusted but revoked certificate reaches no broker staging or VM.
        job = self.intake()
        before = (self.broker_dir / 'requests').stat().st_mtime_ns
        jobs_before = pathlib.Path('/run/26chan-media-jobs').stat().st_mtime_ns
        self.write(self.keys / 'authorized', '0' * 64 + '\n', mode=0o644)
        self.finish(self.dispatch(), False)
        self.no_approval(job)
        assert (self.broker_dir / 'requests').stat().st_mtime_ns == before
        assert pathlib.Path('/run/26chan-media-jobs').stat().st_mtime_ns == jobs_before
        self.clean_vm()
        self.write(self.keys / 'authorized', self.fingerprint + '\n', mode=0o644)
        print('PASS revoked authorization creates no staging, VM or approval', flush=True)

        job = self.intake(b'not an image')
        self.finish(self.dispatch(), False)
        self.no_approval(job)
        self.clean_vm()
        # The stopped decoder disk exists but contains no valid pixel frame.
        assert sql(f"SELECT failure FROM media.jobs WHERE id='{job}'") == 'invalid_output'
        print('PASS actual decoder failure grants no approval', flush=True)

        for mode in ['expired', 'replaced']:
            job = self.intake()
            self.broker.send_signal(signal.SIGSTOP)
            wait_until(lambda: pathlib.Path(f'/proc/{self.broker.pid}/status').read_text().split('State:')[1].lstrip().startswith('T'))
            process = self.dispatch()
            try:
                wait_until(lambda: sql(f"SELECT state FROM media.jobs WHERE id='{job}'") == 'processing')
                if mode == 'expired':
                    sql(f"UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id='{job}'")
                else:
                    sql(f"UPDATE media.jobs SET lease_token=replace(gen_random_uuid()::text,'-','') WHERE id='{job}'")
            finally:
                self.broker.send_signal(signal.SIGCONT)
            self.finish(process, False)
            self.no_approval(job)
            self.clean_vm()
            assert sql(f"SELECT state FROM media.jobs WHERE id='{job}'") == 'processing'
            self.retire(job)
        print('PASS deterministic expired/replaced lease barriers reject actual delayed decoded output', flush=True)

        self.stop(self.broker)
        self.broker = self.start_broker(self.probe)
        job = self.intake(b'sleep')
        process = self.dispatch()
        live = []
        def live_vmm():
            for entry in pathlib.Path('/proc').glob('[0-9]*'):
                try:
                    if (entry / 'comm').read_text().strip() == 'firecracker' and '26chan-media-' in (entry / 'cgroup').read_text():
                        live.append(entry)
                        return True
                except (FileNotFoundError, ProcessLookupError):
                    pass
            return False
        wait_until(live_vmm)
        self.stop(self.broker)
        self.finish(process, False)
        self.no_approval(job)
        assert not live[0].exists(), 'owned cancelled VMM remains'
        self.clean_vm()
        print('PASS cancellation during live VMM stops service and removes VM/broker workspaces', flush=True)

    def cleanup(self):
        for process in reversed(self.processes):
            self.stop(process)
        # Never delete a workspace while a managed VM might still be active.
        if (self.broker_dir / 'requests').exists():
            self.clean_vm()
        for job in self.ids:
            assert HEX.fullmatch(job)
            sql(f"DELETE FROM media.assets WHERE job_id='{job}'; DELETE FROM media.jobs WHERE id='{job}'")
        assert self.root.parent == pathlib.Path('/run') and re.fullmatch('26chan-dispatch-[a-z0-9_]{8}', self.root.name)
        shutil.rmtree(self.root)
        assert all(process.poll() is not None for process in self.processes)


if __name__ == '__main__':
    os.umask(0o077)
    def cancelled(_signal, _frame):
        raise KeyboardInterrupt('owned dispatch qualification cancelled')
    signal.signal(signal.SIGTERM, cancelled)
    exercise = Exercise()
    try:
        exercise.setup()
        exercise.exercise()
    finally:
        exercise.cleanup()
    print('PASS all owned dispatch processes, queue fixtures and private files cleaned')
