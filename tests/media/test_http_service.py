#!/usr/bin/env python3
"""Serve real dispatched output under a separate owned read-only service identity."""
import errno
import hashlib
import http.client
import os
import pathlib
import re
import shutil
import signal
import socket
import subprocess
import urllib.parse
import uuid

from dispatch_service_fixture import SystemdExercise, UnitProcess, systemctl
from test_dispatch import HEX, REPO, SAFE, account, sql, wait_until


class ReaderUnit(UnitProcess):
    def __init__(self, token):
        assert re.fullmatch('[a-z0-9_]{8}', token)
        self.name = f'paperboard-dispatch-qualification-{token}-reader.service'


class MediaHttpExercise(SystemdExercise):
    def __init__(self):
        super().__init__()
        self.http_user = account('board-media-reader')
        assert len({0, self.http_user.pw_uid, self.coordinator.pw_uid,
                    self.gateway.pw_uid, self.vmm.pw_uid}) == 5
        self.http_unit = ReaderUnit(self.root.name.removeprefix('26chan-dispatch-'))
        self.http_installed = False

    def setup(self):
        super().setup()
        shutil.copyfile(self.binaries / 'board-media-http', self.bin / 'board-media-http')
        (self.bin / 'board-media-http').chmod(0o755)
        shutil.copyfile(self.binaries / 'media-backfill', self.bin / 'media-backfill')
        (self.bin / 'media-backfill').chmod(0o755)
        self.objects = self.directory('objects', self.coordinator)
        os.chown(self.objects, self.coordinator.pw_uid, self.http_user.pw_gid)
        self.objects.chmod(0o2750)
        self.writer['MEDIA_GROUP_READ'] = 'true'
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            self.http_port = sock.getsockname()[1]
        self.http_environment()
        text = (REPO / 'deploy/media-http.service').read_text()
        substitutions = {
            '/etc/paperboard/media-reader.env': str(self.root / 'reader.env'),
            '/opt/paperboard/board-media-http': str(self.bin / 'board-media-http'),
            'WorkingDirectory=/opt/paperboard': 'WorkingDirectory=' + str(self.root),
            '-/etc/26chan-coordinator -/var/lib/26chan-coordinator -/etc/26chan-dispatch -/var/lib/26chan-media':
                ' '.join('-' + str(path) for path in (self.private, self.keys, self.media, self.hidden)),
        }
        for before, after in substitutions.items():
            assert text.count(before) == 1, 'candidate path substitution changed'
            text = text.replace(before, after)
        self.unit_file(self.http_unit, text.encode())
        self.processes.append(self.http_unit)
        self.http_installed = True
        systemctl('daemon-reload')
        verified = subprocess.run(['systemd-analyze', 'verify', str(pathlib.Path('/run/systemd/system') / self.http_unit.name)],
                                  env=SAFE, capture_output=True, timeout=15)
        assert verified.returncode == 0, 'rendered reader unit verification failed'
        systemctl('start', self.http_unit.name)
        wait_until(self.ready)

    def http_environment(self, mode='development', extra=''):
        credential = os.environ['MEDIA_READ_DATABASE_URL']
        assert not any(char in credential for char in '\n\r"\\'), 'unsupported fixture credential encoding'
        self.write(self.root / 'reader.env',
                   f'APP_ENV={mode}\nMEDIA_READ_DATABASE_URL="{credential}"\n'
                   f'MEDIA_APPROVED_DIR={self.objects}\nMEDIA_ORIGIN=http://127.0.0.1:{self.http_port}\n'
                   f'MEDIA_BIND_ADDR=127.0.0.1:{self.http_port}\n' + extra)

    def http(self, path, method='GET', headers=None):
        connection = http.client.HTTPConnection('127.0.0.1', self.http_port, timeout=4)
        try:
            connection.request(method, path, headers=headers or {})
            response = connection.getresponse()
            body = response.read(5_242_881)
            assert len(body) <= 5_242_880
            return response.status, dict(response.getheaders()), body
        finally:
            connection.close()

    def ready(self):
        assert self.http_unit.poll() is None, 'reader service startup rejected'
        try:
            return self.http('/readyz')[0] == 200
        except OSError:
            return False

    def dispatch(self):
        return self.launch([self.bin / 'media-publish', 'dispatch', self.private / 'client.json', self.objects],
                           self.coordinator, self.writer)

    def access(self, path, write=False, namespace=True, user=None):
        user = user or self.http_user
        prefix = ['/usr/bin/nsenter', '--target', str(self.http_unit.pid), '--mount', '--root', '--wd=/', '--'] if namespace else []
        operation = 'p.open("r+b").close()' if write else 'p.read_bytes()'
        code = ('import pathlib,sys\np=pathlib.Path(sys.argv[1])\n'
                f'try:\n {operation}\nexcept OSError as error:\n sys.exit(error.errno)\n')
        args = [*prefix, '/usr/bin/setpriv', f'--reuid={user.pw_uid}', f'--regid={user.pw_gid}',
                '--clear-groups', '/usr/bin/python3', '-c', code, str(path)]
        return subprocess.run(args, env=SAFE, capture_output=True, timeout=5).returncode

    def reader_sql(self, statement, allowed):
        url = urllib.parse.urlparse(os.environ['MEDIA_READ_DATABASE_URL'])
        environment = {**SAFE, 'PGHOST': url.hostname, 'PGPORT': str(url.port or 5432),
                       'PGUSER': urllib.parse.unquote(url.username),
                       'PGPASSWORD': urllib.parse.unquote(url.password),
                       'PGDATABASE': urllib.parse.unquote(url.path.lstrip('/'))}
        prefix = ['/usr/bin/nsenter', '--target', str(self.http_unit.pid), '--mount', '--root', '--wd=/', '--',
                  '/usr/bin/setpriv', f'--reuid={self.http_user.pw_uid}', f'--regid={self.http_user.pw_gid}', '--clear-groups']
        result = subprocess.run([*prefix, '/usr/lib/postgresql/16/bin/psql', '-XAt', '-v', 'ON_ERROR_STOP=1'],
                                input='\\set VERBOSITY sqlstate\n' + statement, text=True,
                                env=environment, capture_output=True, timeout=5)
        assert (result.returncode == 0) == allowed, 'reader database permission differs'
        if not allowed:
            assert '42501' in result.stderr, 'denial must be insufficient privilege, not an unavailable database'

    def boundaries(self, asset):
        process = pathlib.Path('/proc') / str(self.http_unit.pid)
        status = dict(line.split(':', 1) for line in (process / 'status').read_text().splitlines())
        assert set(status['Uid'].split()) == {str(self.http_user.pw_uid)}
        assert set(status['Gid'].split()) == {str(self.http_user.pw_gid)}
        assert set(status['Groups'].split()) <= {str(self.http_user.pw_gid)}
        assert int(status['CapEff'], 16) == 0 and status['NoNewPrivs'].strip() == '1'
        environment = dict(item.split(b'=', 1) for item in (process / 'environ').read_bytes().split(b'\0') if item)
        assert environment[b'MEDIA_READ_DATABASE_URL'].decode() == os.environ['MEDIA_READ_DATABASE_URL']
        for name in (b'DATABASE_URL', b'MEDIA_DATABASE_URL', b'STAFF_DATABASE_URL', b'AUTH_DATABASE_URL', b'MIGRATION_DATABASE_URL'):
            assert name not in environment
        self.kernel_limits(self.http_unit, process, 268435456, 32)
        png = self.objects / f'{asset}.png'
        before = hashlib.sha256(png.read_bytes()).digest()
        assert png.stat().st_mode & 0o777 == 0o640 and png.stat().st_gid == self.http_user.pw_gid
        assert self.access(png) == 0
        assert self.access(png, write=True) in (errno.EACCES, errno.EROFS)
        assert self.access(png, write=True, namespace=False, user=self.coordinator) == 0
        for path in (self.private / 'client.key', self.private / 'writer.credential',
                     self.private / 'reader.credential', self.keys / 'server.key'):
            assert path.read_bytes(), 'private witness must exist'
            assert self.access(path) == errno.EACCES
        assert self.access(self.policy / 'writable', write=True, namespace=False) == 0
        assert self.access(self.policy / 'writable', write=True) == errno.EROFS
        assert self.access(self.policy / 'writable', write=True, namespace=False) == 0
        assert hashlib.sha256(png.read_bytes()).digest() == before
        self.reader_sql(f"SELECT id FROM media.approved_assets WHERE id='{asset}';", True)
        for query in ('SELECT count(*) FROM content.boards;', 'SELECT count(*) FROM staff_identity.accounts;',
                      f"UPDATE media.assets SET updated_at=updated_at WHERE id='{asset}';"):
            sql(query)  # Healthy authorized control for the exact relation/operation.
            self.reader_sql(query, False)
            sql(query)
        self.reader_sql(f"SELECT id FROM media.approved_assets WHERE id='{asset}';", True)
        print('PASS actual reader identity, capabilities, kernel limits, read-only storage and protected database/private-file denials with healthy controls', flush=True)

    def legacy_upgrade(self, asset, expected):
        # Construct an old NULL-manifest approval without mutating/downgrading
        # the current approval. Its upgrade uses the real gateway and microVM.
        legacy, job = uuid.uuid4().hex, uuid.uuid4().hex
        self.ids.append(job)
        sql(f"INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at) SELECT '{legacy}','{job}','{uuid.uuid4().hex}',sha256,bytes,width,height,'approved',clock_timestamp() FROM media.assets WHERE id='{asset}';")
        original = self.objects / f'{legacy}.png'
        self.write(original, expected, mode=0o640)
        os.chown(original, self.coordinator.pw_uid, self.http_user.pw_gid)
        assert self.http(f'/media/{legacy}.png')[2] == expected
        assert self.http(f'/media/{legacy}.thumb.png')[0] == 404
        environment = {**SAFE, 'APP_ENV': 'development', 'MEDIA_GROUP_READ': 'true',
                       'MIGRATION_DATABASE_URL': os.environ['MIGRATION_DATABASE_URL']}
        command = [self.bin / 'media-backfill', self.private / 'client.json', self.objects,
                   self.private / 'quarantine', legacy]
        result = self.finish(self.launch(command, env=environment))
        assert result.strip() == b'legacy manifest upgraded'
        self.clean_vm()
        assert original.read_bytes() == expected
        assert self.http(f'/media/{legacy}.png')[2] == expected
        status, headers, thumbnail = self.http(f'/media/{legacy}.thumb.png')
        assert status == 200 and headers['content-type'] == 'image/png'
        assert sql(f"SELECT md5 FROM media.assets WHERE id='{legacy}';") == hashlib.md5(expected).hexdigest()
        assert sql(f"SELECT thumbnail_sha256 FROM media.assets WHERE id='{legacy}';") == hashlib.sha256(thumbnail).hexdigest()
        assert sql(f"SELECT (a.md5,a.thumbnail_sha256,a.thumbnail_bytes,a.thumbnail_width,a.thumbnail_height)=(b.md5,b.thumbnail_sha256,b.thumbnail_bytes,b.thumbnail_width,b.thumbnail_height) FROM media.assets a,media.assets b WHERE a.id='{legacy}' AND b.id='{asset}';") == 't'
        result = self.finish(self.launch(command, env=environment))
        assert result.strip() == b'manifest already current; files verified'
        self.clean_vm()
        assert self.access(self.objects / f'{legacy}.thumb.png') == 0
        assert self.access(self.objects / f'{legacy}.thumb.png', write=True) in (errno.EACCES, errno.EROFS)
        print('PASS legacy NULL manifest -> authenticated real Firecracker decoding -> exact original verification -> thumbnail upgrade -> read-only HTTP; retry preserves original', flush=True)

    def exercise(self):
        job = self.intake()
        asset = self.finish(self.dispatch()).decode().strip()
        assert HEX.fullmatch(asset)
        self.clean_vm()
        png = self.objects / f'{asset}.png'
        expected = png.read_bytes()
        status, headers, body = self.http(f'/media/{asset}.png')
        assert status == 200 and body == expected and headers['content-type'] == 'image/png'
        assert headers['cache-control'] == 'public, no-cache, must-revalidate'
        assert headers['x-content-type-options'] == 'nosniff' and headers['cross-origin-resource-policy'] == 'cross-origin'
        assert 'set-cookie' not in headers and 'access-control-allow-credentials' not in headers
        etag = headers['etag']
        assert self.http(f'/media/{asset}.png', 'HEAD')[2] == b''
        assert self.http(f'/media/{asset}.png', headers={'If-None-Match': etag})[0] == 304
        self.boundaries(asset)
        self.legacy_upgrade(asset, expected)
        pending = uuid.uuid4().hex
        pending_job = uuid.uuid4().hex
        self.ids.append(pending_job)
        sql(f"INSERT INTO media.assets (id,job_id,lease_token,sha256,bytes,width,height) SELECT '{pending}','{pending_job}','{uuid.uuid4().hex}',sha256,bytes,width,height FROM media.assets WHERE id='{asset}';")
        pending_file = self.objects / f'{pending}.png'
        self.write(pending_file, expected, mode=0o640)
        os.chown(pending_file, self.coordinator.pw_uid, self.http_user.pw_gid)
        assert self.access(pending_file) == 0
        assert self.http(f'/media/{pending}.png')[0] == 404
        png.write_bytes(b'X' * len(expected))
        assert self.http(f'/media/{asset}.png', headers={'If-None-Match': etag})[0] == 503
        png.write_bytes(expected)
        sql(f"DELETE FROM media.assets WHERE id='{asset}';")
        assert self.http(f'/media/{asset}.png', headers={'If-None-Match': etag})[0] == 404
        assert self.http('/readyz')[0] == 200
        print('PASS actual queue -> authenticated dispatch -> Firecracker -> approval -> separately identified HTTP PNG reader; pending/corrupt/removed approval rejected', flush=True)
        for mode, extra in [('production', ''), ('development', 'DATABASE_URL=synthetic-forbidden\n')]:
            self.stop(self.http_unit)
            prior = self.http_unit.state()['InvocationID']
            self.http_environment(mode, extra)
            systemctl('reset-failed', self.http_unit.name, required=False)
            systemctl('start', self.http_unit.name, required=False)
            wait_until(lambda: self.http_unit.poll() is not None)
            state = self.http_unit.state()
            assert state['InvocationID'] and state['InvocationID'] != prior
            assert state['Result'] == 'exit-code' and state['ExecMainStatus'] == '1'
            assert state['MainPID'] == '0'
        print('PASS actual service startup rejects production mode and inherited application credentials', flush=True)

    def cleanup(self):
        if self.http_installed:
            self.stop(self.http_unit)
        super().cleanup()
        if self.http_installed:
            systemctl('reset-failed', self.http_unit.name, required=False)


if __name__ == '__main__':
    os.umask(0o077)
    def cancelled(_signal, _frame):
        raise KeyboardInterrupt('owned HTTP qualification cancelled')
    signal.signal(signal.SIGTERM, cancelled)
    exercise = MediaHttpExercise()
    try:
        exercise.setup()
        exercise.exercise()
    finally:
        exercise.cleanup()
    print('PASS all owned HTTP/dispatch units, processes, database fixtures and files cleaned')
