#!/usr/bin/env python3
"""Owned HTTP intake -> authenticated Firecracker -> approved HTTP reader."""
import errno
import http.client
import json
import os
import pathlib
import re
import secrets
import shutil
import signal
import socket
import subprocess
import sys
import urllib.parse

from dispatch_service_fixture import UnitProcess, systemctl
from owned_process import cancel_test, protected_cleanup
from public_upload_fixture import PublicUpload
from test_dispatch import HEX, PG, REPO, SAFE, account, sql, wait_until
from test_http_service import MediaHttpExercise
from test_vm import red_png


class IntakeUnit(UnitProcess):
    def __init__(self, token):
        assert re.fullmatch('[a-z0-9_]{8}', token)
        self.name = f'paperboard-dispatch-qualification-{token}-intake.service'


class IntakeExercise(MediaHttpExercise):
    def __init__(self):
        super().__init__()
        self.intake_user = account('board-media-intake')
        assert self.intake_user.pw_uid not in (0, self.coordinator.pw_uid, self.gateway.pw_uid,
                                             self.vmm.pw_uid, self.http_user.pw_uid)
        self.intake_unit = IntakeUnit(self.root.name.removeprefix('26chan-dispatch-'))
        self.intake_installed = False
        self.service_token = secrets.token_hex(32)
        self.metrics_token = secrets.token_hex(32)
        self.capabilities = []
        self.connections = []
        self.public_upload = PublicUpload(self)

    def setup(self):
        super().setup()
        shutil.copyfile(self.binaries / 'board-media-intake', self.bin / 'board-media-intake')
        (self.bin / 'board-media-intake').chmod(0o755)
        self.quarantine = self.directory('quarantine', self.intake_user)
        self.writer['MEDIA_QUARANTINE_DIR'] = str(self.quarantine)
        # TLS configuration accepts only root or the caller as file owner.
        # Give the development operator its own private copy, preserving the
        # coordinator's existing credential files and ownership.
        self.operator = self.directory('operator')
        for name in ('ca.pem', 'client.pem', 'client.key'):
            self.write(self.operator / name, (self.private / name).read_bytes())
        client = {**self.client_config, 'server_ca': str(self.operator / 'ca.pem'),
                  'client_certificate': str(self.operator / 'client.pem'),
                  'client_key': str(self.operator / 'client.key')}
        self.write(self.operator / 'client.json', json.dumps(client))
        ports = []
        for _ in range(2):
            sock = socket.socket()
            sock.bind(('127.0.0.1', 0))
            ports.append(sock)
        self.intake_port, self.metrics_port = [s.getsockname()[1] for s in ports]
        for sock in ports:
            sock.close()
        credential = os.environ['INTAKE_DATABASE_URL']
        assert not any(c in credential for c in '\n\r"\\')
        self.write(self.root / 'intake.env',
                   f'MEDIA_INTAKE_MODE=development\nMEDIA_INTAKE_BIND=127.0.0.1:{self.intake_port}\n'
                   f'INTAKE_DATABASE_URL="{credential}"\nMEDIA_QUARANTINE_DIR={self.quarantine}\n'
                   f'MEDIA_INTAKE_TOKEN={self.service_token}\nMETRICS_BIND_ADDR=127.0.0.1:{self.metrics_port}\n'
                   f'METRICS_TOKEN={self.metrics_token}\n')
        text = (REPO / 'deploy/media-intake.service').read_text()
        substitutions = {
            '/etc/paperboard/media-intake.env': str(self.root / 'intake.env'),
            '/opt/paperboard/board-media-intake': str(self.bin / 'board-media-intake'),
            'WorkingDirectory=/opt/paperboard': 'WorkingDirectory=' + str(self.root),
            '/var/lib/paperboard/quarantine': str(self.quarantine),
            '-/etc/26chan-coordinator -/var/lib/26chan-coordinator -/etc/26chan-dispatch -/var/lib/26chan-media':
                ' '.join('-' + str(p) for p in (self.private, self.keys, self.media, self.hidden, self.objects, self.operator)),
        }
        for before, after in substitutions.items():
            assert text.count(before) == 1, 'candidate substitution changed'
            text = text.replace(before, after)
        self.unit_file(self.intake_unit, text.encode())
        self.processes.append(self.intake_unit)
        self.intake_installed = True
        systemctl('daemon-reload')
        result = subprocess.run(['systemd-analyze', 'verify', '/run/systemd/system/' + self.intake_unit.name],
                                env=SAFE, capture_output=True, timeout=15)
        assert result.returncode == 0, 'intake candidate verification failed'
        systemctl('start', self.intake_unit.name)
        wait_until(self.intake_ready)
        self.public_upload.setup()

    def intake_ready(self):
        assert self.intake_unit.poll() is None, 'intake service startup rejected'
        try:
            return self.call('/readyz')[0] == 200
        except OSError:
            return False

    def call(self, path, method='GET', body=None, headers=None, authenticated=True, chunked=False):
        connection = http.client.HTTPConnection('127.0.0.1', self.intake_port, timeout=23)
        request_headers = {'Authorization': 'Bearer ' + self.service_token} if authenticated else {}
        request_headers.update(headers or {})
        try:
            connection.request(method, path, body=body, headers=request_headers, encode_chunked=chunked)
            response = connection.getresponse()
            data = response.read(2049)
            assert len(data) <= 2048
            assert response.getheader('Cache-Control') == 'private, no-store'
            assert response.getheader('X-Content-Type-Options') == 'nosniff'
            assert response.getheader('Access-Control-Allow-Origin') is None
            return response.status, json.loads(data)
        finally:
            connection.close()

    def reserve_http(self):
        status, result = self.call('/v1/reservations', 'POST', b'{"filename":"synthetic.png"}',
                                   {'Content-Type': 'application/json'})
        assert status == 201 and result['state'] == 'receiving'
        job, cap = result['id'], result['capability']
        assert HEX.fullmatch(job) and re.fullmatch('[0-9a-f]{64}', cap)
        self.ids.append(job)
        self.capabilities.append(cap)
        return job, cap

    def upload_http(self, job, cap, body, chunked=False):
        return self.call(f'/v1/uploads/{job}', 'PUT', body,
                         {'Content-Type': 'application/octet-stream', 'Upload-Capability': cap}, chunked=chunked)

    def dispatch(self):
        # The development operator reads intake-owned 0700 storage. This does
        # not qualify a future deployed coordinator's storage access policy.
        return self.launch([self.bin / 'media-publish', 'dispatch', self.operator / 'client.json', self.objects],
                           env=self.writer)

    def incomplete_upload(self):
        job, cap = self.reserve_http()
        connection = http.client.HTTPConnection('127.0.0.1', self.intake_port, timeout=23)
        self.connections.append(connection)
        connection.putrequest('PUT', f'/v1/uploads/{job}')
        for key, value in {'Authorization': 'Bearer ' + self.service_token, 'Upload-Capability': cap,
                           'Content-Type': 'application/octet-stream', 'Content-Length': '32'}.items():
            connection.putheader(key, value)
        connection.endheaders(b'partial')
        wait_until(lambda: (self.quarantine / f'{job}.part').exists())
        return connection, job, cap

    def intake_access(self, path, write=False, namespace=True):
        original = self.http_unit
        try:
            self.http_unit = self.intake_unit
            return self.access(path, write=write, namespace=namespace, user=self.intake_user)
        finally:
            self.http_unit = original

    def intake_sql(self, statement, allowed):
        url = urllib.parse.urlparse(os.environ['INTAKE_DATABASE_URL'])
        env = {**SAFE, 'PGHOST': url.hostname, 'PGPORT': str(url.port or 5432),
               'PGUSER': urllib.parse.unquote(url.username), 'PGPASSWORD': urllib.parse.unquote(url.password),
               'PGDATABASE': urllib.parse.unquote(url.path.lstrip('/'))}
        prefix = ['/usr/bin/nsenter', '--target', str(self.intake_unit.pid), '--mount', '--root', '--wd=/', '--',
                  '/usr/bin/setpriv', f'--reuid={self.intake_user.pw_uid}', f'--regid={self.intake_user.pw_gid}', '--clear-groups']
        result = subprocess.run([*prefix, PG, '-XAt', '-v', 'ON_ERROR_STOP=1'],
                                input='\\set VERBOSITY sqlstate\n' + statement, text=True,
                                env=env, capture_output=True, timeout=5)
        assert (result.returncode == 0) == allowed, 'intake database permission differs'
        if not allowed:
            assert '42501' in result.stderr, 'denial requires a healthy database'

    def boundaries(self, job, asset):
        process = pathlib.Path('/proc') / str(self.intake_unit.pid)
        status = dict(line.split(':', 1) for line in (process / 'status').read_text().splitlines())
        assert set(status['Uid'].split()) == {str(self.intake_user.pw_uid)}
        assert set(status['Gid'].split()) == {str(self.intake_user.pw_gid)}
        assert set(status['Groups'].split()) <= {str(self.intake_user.pw_gid)}
        assert int(status['CapEff'], 16) == 0 and status['NoNewPrivs'].strip() == '1'
        environment = dict(item.split(b'=', 1) for item in (process / 'environ').read_bytes().split(b'\0') if item)
        assert environment[b'INTAKE_DATABASE_URL'].decode() == os.environ['INTAKE_DATABASE_URL']
        for name in ('DATABASE_URL', 'MEDIA_DATABASE_URL', 'MEDIA_READ_DATABASE_URL', 'AUTH_DATABASE_URL',
                     'STAFF_DATABASE_URL', 'MIGRATION_DATABASE_URL', 'AWS_ACCESS_KEY_ID', 'SSH_AUTH_SOCK'):
            assert name.encode() not in environment
        self.kernel_limits(self.intake_unit, process, 268435456, 32)
        witness = self.quarantine / 'owned-witness'
        self.write(witness, b'healthy intake storage', self.intake_user)
        assert self.intake_access(witness, write=True) == 0
        for path in (self.private / 'writer.credential', self.private / 'client.key', self.operator / 'client.key',
                     self.keys / 'server.key', self.objects / f'{asset}.png', self.root / 'reader.env'):
            assert path.read_bytes(), 'healthy protected file witness missing'
            assert self.intake_access(path) == errno.EACCES
        assert self.intake_access(self.policy / 'writable', write=True, namespace=False) == 0
        assert self.intake_access(self.policy / 'writable', write=True) == errno.EROFS
        assert self.intake_access(self.policy / 'writable', write=True, namespace=False) == 0
        self.intake_sql('SELECT media_intake.ready();', True)
        for query in ('SELECT count(*) FROM content.boards;', 'SELECT count(*) FROM staff_identity.accounts;',
                      'SELECT count(*) FROM media.jobs;', 'SELECT count(*) FROM media_intake.handles;',
                      f"UPDATE media.assets SET updated_at=updated_at WHERE id='{asset}';"):
            sql(query)
            self.intake_sql(query, False)
            sql(query)
        self.intake_sql('SELECT media_intake.ready();', True)
        print('PASS actual intake UID, absent unrelated credentials, kernel limits, private storage and database denials with healthy controls', flush=True)

    def exercise(self):
        assert self.call('/healthz', authenticated=False)[0] == 401
        assert self.call('/healthz', headers={'Origin': 'null'})[0] == 403
        assert self.call('/v1/reservations', 'POST', b'x' * 1025, {'Content-Type': 'application/json'})[0] == 413
        job, cap = self.reserve_http()
        assert self.upload_http(job, '0' * 64, b'denied')[0] == 404
        assert self.upload_http(job, cap, iter([red_png()]), chunked=True)[0] == 202
        assert self.upload_http(job, cap, b'duplicate')[0] == 409
        asset = self.finish(self.dispatch()).decode().strip()
        assert HEX.fullmatch(asset)
        self.clean_vm()
        status, result = self.call(f'/v1/uploads/{job}', headers={'Upload-Capability': cap})
        assert status == 200 and result['state'] == 'published' and result['output_id'] == asset
        assert self.http(f'/media/{asset}.png')[2] == (self.objects / f'{asset}.png').read_bytes()
        self.boundaries(job, asset)
        print('PASS HTTP reservation/upload -> authenticated Firecracker dispatch -> fenced approval -> capability status -> separate HTTP reader', flush=True)
        self.public_upload.exercise()

        job, cap = self.reserve_http()
        assert self.upload_http(job, cap, iter([b'x' * 8192] * 1025), chunked=True)[0] == 413
        assert not (self.quarantine / f'{job}.input').exists()
        assert not (self.quarantine / f'{job}.part').exists()
        connection, job, cap = self.incomplete_upload()
        assert self.upload_http(job, cap, b'duplicate')[0] == 409
        connection.close()
        wait_until(lambda: not (self.quarantine / f'{job}.part').exists())
        assert not (self.quarantine / f'{job}.input').exists()
        connection, job, cap = self.incomplete_upload()
        response = connection.getresponse()
        assert response.status == 408
        response.read()
        connection.close()
        assert not (self.quarantine / f'{job}.part').exists()
        print('PASS actual chunked overflow, duplicate writer, disconnected transfer and 15-second receive deadline cleanup', flush=True)

        connection = http.client.HTTPConnection('127.0.0.1', self.metrics_port, timeout=4)
        try:
            connection.request('GET', '/metrics', headers={'Authorization': 'Bearer ' + self.metrics_token})
            response = connection.getresponse()
            body = response.read(65537)
            assert response.status == 200 and len(body) <= 65536 and b'listener="intake"' in body
            assert self.service_token.encode() not in body
        finally:
            connection.close()
        journal = subprocess.run(['journalctl', '-u', self.intake_unit.name, '--no-pager', '-o', 'cat'],
                                 env=SAFE, capture_output=True, timeout=5)
        assert journal.returncode == 0
        for value in [self.service_token, self.metrics_token, 'synthetic.png', *self.capabilities]:
            assert value.encode() not in journal.stdout + journal.stderr
        self.intake_unit.send_signal(signal.SIGTERM)
        wait_until(lambda: self.intake_unit.poll() is not None)
        assert self.intake_unit.poll() == 0
        self.closed_ports()
        print('PASS secret-free fixed-label metrics/logs and SIGTERM listener shutdown', flush=True)

    def closed_ports(self):
        for port in (self.intake_port, self.metrics_port):
            with socket.socket() as sock:
                assert sock.connect_ex(('127.0.0.1', port)) != 0

    def cleanup(self):
        self.public_upload.cleanup()
        for connection in self.connections:
            connection.close()
        if self.intake_installed:
            self.stop(self.intake_unit)
        super().cleanup()
        if self.intake_installed:
            self.closed_ports()
            systemctl('reset-failed', self.intake_unit.name, required=False)


if __name__ == '__main__':
    os.umask(0o077)
    assert sys.argv[1:] in ([], ['--interrupt'])
    interrupt = bool(sys.argv[1:])
    handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGTERM, signal.SIGINT)}
    for sig in handlers:
        signal.signal(sig, cancel_test)
    exercise = IntakeExercise()
    interrupted = False
    try:
        exercise.setup()
        if interrupt:
            exercise.incomplete_upload()
            os.kill(os.getpid(), signal.SIGTERM)
            raise AssertionError('qualification signal was not delivered')
        exercise.exercise()
    except KeyboardInterrupt:
        if not interrupt:
            raise
        interrupted = True
    finally:
        with protected_cleanup(handlers):
            exercise.cleanup()
    assert not exercise.root.exists()
    assert all(not path.exists() for path in exercise.unit_files)
    assert interrupted == interrupt
    print('PASS all owned intake/reader/dispatch services, processes, sockets, database rows and private files cleaned', flush=True)
