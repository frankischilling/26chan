"""Development public web unit connected to real intake, dispatch and reader."""
import http.client
import os
import pwd
import re
import secrets
import shutil
import socket
import subprocess

from dispatch_service_fixture import UnitProcess, systemctl
from test_dispatch import HEX, REPO, SAFE, account, sql, wait_until
from test_vm import red_png


class PublicUnit(UnitProcess):
    def __init__(self, token):
        assert re.fullmatch('[a-z0-9_]{8}', token)
        self.name = f'paperboard-dispatch-qualification-{token}-public.service'


class PublicUpload:
    def __init__(self, fixture):
        self.f = fixture
        token = fixture.root.name.removeprefix('26chan-dispatch-')
        self.board = 'u' + secrets.token_hex(4)
        self.unit = PublicUnit(token)
        self.installed = False
        self.created = False
        self.browser = None
        self.filenames = []

    def setup(self):
        f = self.f
        public = account('board-public')
        assert public.pw_uid not in (0, f.intake_user.pw_uid, f.coordinator.pw_uid, f.http_user.pw_uid)
        shutil.copyfile(f.binaries / 'board-public', f.bin / 'board-public')
        (f.bin / 'board-public').chmod(0o755)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            self.port = sock.getsockname()[1]
        self.origin = f'http://127.0.0.1:{self.port}'
        credential = os.environ['TEST_PUBLIC_DATABASE_URL']
        assert not any(c in credential for c in '\n\r"\\')
        f.write(f.root / 'public.env',
                f'APP_ENV=development\nMEDIA_ENABLED=true\nPUBLIC_MEDIA_PROFILE=isolated-development\n'
                f'DATABASE_URL="{credential}"\nBIND_ADDR=127.0.0.1:{self.port}\nPUBLIC_ORIGIN={self.origin}\n'
                f'STAFF_ORIGIN=http://localhost:3001\nMEDIA_ORIGIN=http://127.0.0.1:{f.http_port}\n'
                f'PUBLIC_INTAKE_ADDR=127.0.0.1:{f.intake_port}\nPUBLIC_INTAKE_TOKEN={f.service_token}\n')
        text = (REPO / 'deploy/public.service').read_text()
        for before, after in {
            '/etc/paperboard/public.env': str(f.root / 'public.env'),
            '/opt/paperboard/board-public': str(f.bin / 'board-public'),
            'WorkingDirectory=/opt/paperboard': 'WorkingDirectory=' + str(f.root),
        }.items():
            assert text.count(before) == 1
            text = text.replace(before, after)
        f.unit_file(self.unit, text.encode())
        f.processes.append(self.unit)
        self.installed = True
        systemctl('daemon-reload')
        result = subprocess.run(['systemd-analyze', 'verify', '/run/systemd/system/' + self.unit.name],
                                env=SAFE, capture_output=True, timeout=15)
        assert result.returncode == 0, 'public development unit verification failed'
        sql(f"INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit) VALUES ('{self.board}','Upload qualification','Synthetic PNG and JPEG',2000,100,100,100,10,3);")
        self.created = True
        systemctl('start', self.unit.name)
        wait_until(self.ready)

    def ready(self):
        assert self.unit.poll() is None, 'public development startup rejected'
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=3)
        try:
            connection.request('GET', '/readyz')
            response = connection.getresponse()
            response.read(4096)
            return response.status == 200
        except OSError:
            return False
        finally:
            connection.close()

    def exercise(self):
        cases = [('png', red_png())]
        cases.extend((name + '.jpg', (REPO / 'tests/media/fixtures/jpeg' / (name + '.jpg')).read_bytes())
                     for name in ('baseline', 'progressive'))
        for suffix, data in cases:
            self.upload_one(suffix, data)

    def upload_one(self, suffix, data):
        f = self.f
        # Browser runs as the checkout owner, not root or an application identity.
        # Its cleared environment has no database or service credentials.
        browser_user = pwd.getpwuid(REPO.stat().st_uid)
        assert browser_user.pw_uid != 0, 'checkout must belong to the nonroot test operator'
        filename = f'public-upload-{self.board}.{suffix}'
        self.filenames.append(filename)
        source = f.root / filename
        f.write(source, data, browser_user)
        environment = {**SAFE, 'HOME': browser_user.pw_dir}
        node = os.environ['PUBLIC_UPLOAD_NODE']
        assert os.path.isabs(node) and os.path.isfile(node)
        process = f.launch([node, REPO / 'tests/browser/public-upload.mjs', self.origin, self.board, source],
                           browser_user, environment)
        self.browser = process
        query = f"SELECT j.id FROM media.jobs j WHERE j.filename='{filename}' AND j.state='queued';"
        def queued():
            assert process.poll() is None, 'public upload browser exited before queueing'
            return bool(sql(query))
        wait_until(queued, seconds=25)
        job = sql(query)
        assert HEX.fullmatch(job)
        f.ids.append(job)
        asset = f.finish(f.dispatch()).decode().strip()
        assert HEX.fullmatch(asset)
        f.clean_vm()
        output = f.finish(process)
        assert output.startswith(b'PASS no-JavaScript upload, isolated approval, persisted posting')
        assert sql(f"SELECT count(*) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board='{self.board}' AND m.asset_id='{asset}' AND m.file_deleted AND NOT p.deleted;") == '1'
        assert f.http(f'/media/{asset}.png')[0] == 404
        assert (f.objects / f'{asset}.png').is_file()
        assert (f.objects / f'{asset}.thumb.png').is_file()
        cleaned = f.finish(f.launch([f.bin / 'media-publish', 'reconcile', f.objects], env=f.writer))
        assert int(cleaned.strip()) >= 1
        assert not (f.objects / f'{asset}.png').exists()
        assert not (f.objects / f'{asset}.thumb.png').exists()
        assert sql(f"SELECT count(*) FROM content.post_media WHERE asset_id='{asset}' AND file_deleted;") == '1'
        assert f.http(f'/media/{asset}.png')[0] == 404
        assert f.http(f'/media/{asset}.thumb.png')[0] == 404
        print(f'PASS {suffix}: real nonroot public browser -> authenticated intake -> Firecracker -> persisted attachment -> browser image -> deletion revokes reader -> both files removed with tombstone retained', flush=True)

    def cleanup(self):
        if self.browser is not None:
            self.f.stop(self.browser)
        if self.installed:
            self.f.stop(self.unit)
        if self.created:
            for filename in self.filenames:
                assert re.fullmatch(r'public-upload-u[0-9a-f]{8}\.(png|baseline\.jpg|progressive\.jpg)', filename)
                for job in sql(f"SELECT id FROM media.jobs WHERE filename='{filename}';").splitlines():
                    assert HEX.fullmatch(job)
                    if job not in self.f.ids:
                        self.f.ids.append(job)
            # Remove only this fixture's durable links before parent job cleanup.
            sql(f"DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board='{self.board}'); DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board='{self.board}'); DELETE FROM content.posts WHERE board='{self.board}'; DELETE FROM content.threads WHERE board='{self.board}'; DELETE FROM content.boards WHERE slug='{self.board}';")
