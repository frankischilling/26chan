"""Development public web unit connected to real intake, dispatch and reader."""
import hashlib
import hmac
import http.client
import grp
import os
import pathlib
import pwd
import re
import secrets
import shutil
import socket
import subprocess

from dispatch_service_fixture import UnitProcess, systemctl
from test_dispatch import HEX, REPO, SAFE, account, sql, wait_until
from test_vm import red_png


def finish_browser(process, script):
    # Browser stderr can include one-use capabilities in page/response details.
    # Emit only a fixed script/location and an allowlisted HTTP response category.
    assert script in ('public-upload.mjs', 'quick-reply-upload.mjs', 'drawing-upload.mjs')
    output, error = process.communicate(timeout=35)
    if process.returncode != 0:
        locations = re.findall(rb'/' + re.escape(script.encode()) + rb':([0-9]{1,6}):([0-9]{1,6})\b', error)
        location = ':' + ':'.join(value.decode('ascii') for value in locations[0]) if locations else ''
        responses = re.findall(rb'^OWNED_UPLOAD_RESPONSE status=([1-5][0-9]{2}) type=(json|html|plain|other)\r?$', error, re.MULTILINE)
        response = (' (HTTP ' + responses[0][0].decode('ascii') + ', ' + responses[0][1].decode('ascii') + ')') if responses else ''
        classified = re.findall(rb'^OWNED_UPLOAD_RESPONSE status=([1-5][0-9]{2}) type=(json|html|plain|other) stage=(upload|post|owner-thread) failure=(http|json|body)\r?$', error, re.MULTILINE)
        if classified:
            status, category, stage, failure = (value.decode('ascii') for value in classified[-1])
            response = f' (HTTP {status}, {category}, {stage}, {failure})'
        deletion = re.findall(rb'^OWNED_UPLOAD_RESPONSE status=([1-5][0-9]{2}) type=(json|html|plain|other) stage=deletion failure=(http|body|content)\r?$', error, re.MULTILINE)
        if deletion:
            status, category, failure = (value.decode('ascii') for value in deletion[-1])
            response = f' (HTTP {status}, {category}, deletion, {failure})'
        drawing = re.findall(rb'^OWNED_DRAWING_EDIT (cancel=(?:none|pending|failed|other|[1-5][0-9]{2}) ui=(?:empty|queued|canceling|checking|uploading|cancel-error|editor-error|other) editor=(?:absent|hidden|visible) cursor=(?:absent|hidden|visible) active=(?:true|false|other)|unavailable)\r?$', error, re.MULTILINE)
        if drawing and script == 'drawing-upload.mjs':
            response += ' (Edit ' + drawing[-1].decode('ascii') + ')'
        raise AssertionError('owned upload browser rejected at ' + script + location + response)
    return output


class PublicUnit(UnitProcess):
    def __init__(self, token):
        assert re.fullmatch('[a-z0-9_]{8}', token)
        self.name = f'paperboard-dispatch-qualification-{token}-public.service'


class PublicUpload:
    def __init__(self, fixture):
        self.f = fixture
        token = fixture.root.name.removeprefix('26chan-dispatch-')
        self.board = 'u' + secrets.token_hex(4)
        self._owned_board = self.board
        # Fresh synthetic server key, never inherited from an external environment.
        self._poster_id_key = secrets.token_hex(32)
        self.unit = PublicUnit(token)
        self.installed = False
        self.created = False
        self.browser = None
        self.filenames = []
        self.completed_jobs = []
        self._public_identity = None
        self.drawing_upload = None

    def setup(self):
        f = self.f
        public = account('board-public')
        # The candidate unit now runs with the dedicated proxy socket group.
        # Provision that prerequisite on this owned disposable test host too.
        try:
            edge = grp.getgrnam('board-edge')
        except KeyError:
            subprocess.run(['groupadd', '--system', 'board-edge'], check=True,
                           capture_output=True, env=SAFE)
            edge = grp.getgrnam('board-edge')
        assert edge.gr_gid != 0
        assert public.pw_uid not in (0, f.intake_user.pw_uid, f.coordinator.pw_uid, f.http_user.pw_uid)
        self._public_identity = (public.pw_uid, edge.gr_gid)
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
                f'POSTER_ID_KEY={self._poster_id_key}\n'
                f'DATABASE_URL="{credential}"\nBIND_ADDR=127.0.0.1:{self.port}\nPUBLIC_ORIGIN={self.origin}\n'
                f'STAFF_ORIGIN=http://localhost:3001\nMEDIA_ORIGIN=http://127.0.0.1:{f.http_port}\n'
                # These workflows share one loopback peer; retain enforcement
                # with the same bounded test rate as the persisted browser suite.
                f'PUBLIC_WRITES_PER_MINUTE=60\n'
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
        self.create_board()
        systemctl('start', self.unit.name)
        wait_until(self.ready)
        status = pathlib.Path(f'/proc/{self.unit.pid}/status').read_text()
        ids = dict(line.split(':', 1) for line in status.splitlines() if ':' in line)
        assert set(map(int, ids['Uid'].split())) == {public.pw_uid}
        assert set(map(int, ids['Gid'].split())) == {edge.gr_gid}

    def create_board(self):
        assert self.board == self._owned_board
        assert re.fullmatch(r'u[0-9a-f]{8}', self.board)
        # This owned synthetic board qualifies media deletion and cleanup directly
        # after posting. Source-board age eligibility is tested separately; retain
        # the normal maximum age and all authentication/resource checks here.
        # Zero ordinary timers belong only to this disposable media board.
        sql(f"INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,comment_spoiler_cleanup,deletion_known_min_seconds,deletion_unknown_min_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES ('{self.board}','Upload qualification','Synthetic PNG, JPEG and GIF',2000,100,100,100,10,3,true,0,0,0,0,0);")
        self.created = True

    def _actor_hex(self):
        # Match PosterIdKey::public_deletion_rate_identity exactly: domain,
        # canonical IPv4 family byte, then the real loopback peer's four octets.
        return hmac.new(bytes.fromhex(self._poster_id_key),
                        b'26chan-public-deletion-rate-v1\0\x04\x7f\0\0\x01',
                        hashlib.sha256).hexdigest()

    def _posting_actor_hex(self):
        # Distinct domain, same fresh fixture key and actual IPv4 loopback peer.
        return hmac.new(bytes.fromhex(self._poster_id_key),
                        b'26chan-public-posting-rate-v1\0\x04\x7f\0\0\x01',
                        hashlib.sha256).hexdigest()

    def reset_posting_history(self):
        assert self.created and self.board == self._owned_board
        assert re.fullmatch(r'u[0-9a-f]{8}', self.board)
        assert sql(f"SELECT posting_reply_seconds,posting_image_seconds,posting_thread_seconds FROM content.boards WHERE slug='{self.board}';") == '0|0|0'
        # Only the trusted supervisor can touch private admission state. Both
        # full actor identity and owned board scope must match; no global reset.
        for table in ('posting_history', 'posting_thread_actions'):
            sql(f"DELETE FROM post_secrets.{table} WHERE actor_hash=decode('{self._posting_actor_hex()}','hex') AND board='{self.board}';")

    def reset_deletion_quota(self):
        assert self.created and self.board == self._owned_board
        assert re.fullmatch(r'u[0-9a-f]{8}', self.board)
        assert sql(f"SELECT deletion_known_min_seconds,deletion_unknown_min_seconds,deletion_max_seconds FROM content.boards WHERE slug='{self.board}';") == '0|0|1800'
        # sql() runs only in this trusted owner harness. Neither its credential
        # nor this operation is exposed to the public service/browser.
        sql(f"DELETE FROM post_secrets.public_deletion_actors WHERE actor_hash=decode('{self._actor_hex()}','hex');")

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
        cases = [('png', red_png(), False)]
        cases.extend((name + '.jpg', (REPO / 'tests/media/fixtures/jpeg' / (name + '.jpg')).read_bytes(), False)
                     for name in ('baseline', 'progressive'))
        cases.append(('static.gif', (REPO / 'tests/media/fixtures/gif/static.gif').read_bytes(), False))
        cases.append(('tracking.png', red_png(), True))
        for suffix, data, javascript in cases:
            self.upload_one(suffix, data, javascript)
        self.upload_one('quick-reply.png', red_png(), True, quick_reply=True)
        self.upload_one('quick-reply-inline.png', red_png(), True, quick_reply='inline')
        sql(f"UPDATE content.boards SET comment_spoiler_cleanup=false WHERE slug='{self.board}' AND title='Upload qualification';")
        self.upload_one('quick-reply-disabled.png', red_png(), True, quick_reply=True, spoilers=False)
        self.upload_one('quick-reply-inline-disabled.png', red_png(), True, quick_reply='inline', spoilers=False)
        # Drawing shares this server's actor on a new owned board. Its own reset
        # cannot remove this board's cross-board OP cooldown. End the completed
        # host workflow with the same exact actor-and-owned-board scoped reset.
        self.reset_posting_history()
        self.restart_for_drawing()
        from public_drawing_fixture import PublicDrawingUpload
        self.drawing_upload = PublicDrawingUpload(self)
        self.drawing_upload.exercise()

    def restart_for_drawing(self):
        # Each browser batch owns the same loopback write-budget bucket. Start
        # drawing with a fresh instance of only this disposable public service,
        # after every preceding browser, dispatch and reconcile has completed.
        # Keep the configured 60/minute limit and production policy unchanged.
        assert self.installed and self.created and self.board == self._owned_board
        assert self.drawing_upload is None
        assert self._public_identity and all(isinstance(value, int) and value > 0 for value in self._public_identity)
        token = self.f.root.name.removeprefix('26chan-dispatch-')
        assert re.fullmatch('[a-z0-9_]{8}', token)
        assert self.unit.name == f'paperboard-dispatch-qualification-{token}-public.service'
        assert self.unit in self.f.processes
        path = pathlib.Path('/run/systemd/system') / self.unit.name
        assert path in self.f.unit_files and path.read_bytes() == self.f.unit_files[path]
        assert len(self.filenames) == len(set(self.filenames)) == 9
        assert len(self.completed_jobs) == len(set(self.completed_jobs)) == 9
        assert all(HEX.fullmatch(job) and job in self.f.ids for job in self.completed_jobs)
        assert all(re.fullmatch(r'public-upload-' + self.board + r'\.[a-z.-]+', name) for name in self.filenames)
        assert self.browser is not None and self.browser.poll() == 0, 'previous upload browser is not complete'
        names = ','.join("'" + name + "'" for name in self.filenames)
        jobs = ','.join("'" + job + "'" for job in self.completed_jobs)
        assert sql(f"SELECT count(*)=9 AND bool_and(state='published' AND id IN ({jobs})) FROM media.jobs WHERE filename IN ({names});") == 't', 'previous upload jobs are not complete'
        self.f.clean_vm()
        previous = self.unit.state()
        assert previous['ActiveState'] == 'active' and re.fullmatch('[a-f0-9]{32}', previous['InvocationID'])
        systemctl('restart', self.unit.name, timeout=35)
        wait_until(self.ready)
        current = self.unit.state()
        assert current['ActiveState'] == 'active' and re.fullmatch('[a-f0-9]{32}', current['InvocationID'])
        assert current['InvocationID'] != previous['InvocationID'], 'public service did not start a fresh invocation'
        pid = int(current['MainPID'])
        assert pid > 0
        status = pathlib.Path(f'/proc/{pid}/status').read_text()
        ids = dict(line.split(':', 1) for line in status.splitlines() if ':' in line)
        uid, gid = self._public_identity
        assert set(map(int, ids['Uid'].split())) == {uid}
        assert set(map(int, ids['Gid'].split())) == {gid}
        self.browser = None

    def upload_one(self, suffix, data, javascript=False, quick_reply=False, spoilers=True):
        self.reset_deletion_quota()
        self.reset_posting_history()
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
        # Preserve text-plus-image coverage and also qualify image-only PNG/JPEG/GIF
        # through the actual isolated pipeline, not merely synthetic approvals.
        flags = [] if suffix in ('baseline.jpg', 'tracking.png') else ['--attachment-only']
        if javascript:
            flags.append('--javascript')
        script = 'public-upload.mjs'
        if quick_reply:
            script, flags = 'quick-reply-upload.mjs', (['--inline'] if quick_reply == 'inline' else [])
            if not spoilers:
                flags.append('--no-spoilers')
        process = f.launch([node, REPO / 'tests/browser' / script, self.origin, self.board, source, *flags],
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
        output = finish_browser(process, script)
        assert sql(f"SELECT cardinality(events) FROM post_secrets.public_deletion_actors WHERE actor_hash=decode('{self._actor_hex()}','hex');") == '1'
        expected_posts = 3 if quick_reply else (2 if javascript else 1)
        assert sql(f"SELECT count(*) FROM post_secrets.posting_history WHERE actor_hash=decode('{self._posting_actor_hex()}','hex') AND board='{self.board}';") == str(expected_posts)
        assert sql(f"SELECT count(*) FROM post_secrets.posting_thread_actions WHERE actor_hash=decode('{self._posting_actor_hex()}','hex') AND board='{self.board}';") == '1'
        mode = b'JavaScript' if javascript else b'no-JavaScript'
        assert output.startswith(b'PASS ' + mode + b' upload, isolated approval, persisted posting')
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
        self.completed_jobs.append(job)
        print(f'PASS {suffix}: real nonroot public browser -> authenticated intake -> Firecracker -> persisted attachment -> browser image -> deletion revokes reader -> both files removed with tombstone retained', flush=True)

    def cleanup(self):
        if self.drawing_upload is not None:
            self.drawing_upload.cleanup()
        if self.browser is not None:
            self.f.stop(self.browser)
        if self.installed:
            self.f.stop(self.unit)
        if self.created:
            self.reset_deletion_quota()
            self.reset_posting_history()
            for filename in self.filenames:
                assert re.fullmatch(r'public-upload-u[0-9a-f]{8}\.(png|baseline\.jpg|progressive\.jpg|static\.gif|tracking\.png|quick-reply\.png|quick-reply-inline\.png|quick-reply-disabled\.png|quick-reply-inline-disabled\.png)', filename)
                for job in sql(f"SELECT id FROM media.jobs WHERE filename='{filename}';").splitlines():
                    assert HEX.fullmatch(job)
                    if job not in self.f.ids:
                        self.f.ids.append(job)
            # Remove only this fixture's durable links before parent job cleanup.
            sql(f"DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board='{self.board}'); DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board='{self.board}'); DELETE FROM content.posts WHERE board='{self.board}'; DELETE FROM content.threads WHERE board='{self.board}'; DELETE FROM content.boards WHERE slug='{self.board}';")
