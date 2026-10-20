"""One verified JPEG post on an exclusively leased disposable /p/ board."""
import http.client
import json
import os
import pwd
import re
import secrets
import shutil
import socket
import subprocess

from dispatch_service_fixture import systemctl
from public_source_gif_fixture import PublicSourceGif
from test_dispatch import HEX, REPO, SAFE, account, sql, wait_until


class PublicVipJpeg:
    def __init__(self, host):
        self.host, self.f = host, host.f
        self.filename = 'public-vip-jpeg-' + secrets.token_hex(16) + '.jpg'
        self.identifier = 'ownedjpeg-' + secrets.token_hex(8)
        self.password = secrets.token_hex(16)
        self.key = secrets.token_hex(32)
        self.token = secrets.token_hex(32)
        self.mail_key = secrets.token_hex(32)
        self.policy = self.public_env = None
        self.issuer = self.browser = None
        self.account_id = None

    def lease_board(self):
        assert self.policy is None
        assert re.fullmatch(r'public-vip-jpeg-[0-9a-f]{32}\.jpg', self.filename)
        before = json.loads(sql("SELECT to_jsonb(b) FROM content.boards b WHERE slug='p';"))
        assert before['slug'] == 'p' and before['staff_only'] is False and before['captcha_enabled'] is True
        active = {**before, 'description': self.filename, 'captcha_enabled': False}
        self.policy = {'before': before, 'active': active}
        original, expected = map(PublicSourceGif._json_sql, (before, active))
        sql(f"""BEGIN; SET LOCAL standard_conforming_strings=on; SET LOCAL lock_timeout='1s';
            DO $owned_vip$ BEGIN
                PERFORM 1 FROM content.boards WHERE slug='p' FOR UPDATE NOWAIT;
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='p') IS DISTINCT FROM {original}
                OR EXISTS(SELECT 1 FROM content.posts WHERE board='p')
                OR EXISTS(SELECT 1 FROM content.threads WHERE board='p')
                OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE board='p')
                OR EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions WHERE board='p')
                THEN RAISE EXCEPTION 'VIP JPEG requires an untouched disposable board'; END IF;
                UPDATE content.boards SET description='{self.filename}',captcha_enabled=false WHERE slug='p';
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='p') IS DISTINCT FROM {expected}
                THEN RAISE EXCEPTION 'VIP JPEG changed unrelated policy'; END IF;
            END $owned_vip$; COMMIT;""")

    def restore_board(self):
        if self.policy is None:
            return
        original, expected = map(PublicSourceGif._json_sql, (self.policy['before'], self.policy['active']))
        sql(f"""BEGIN; SET LOCAL standard_conforming_strings=on; SET LOCAL lock_timeout='1s';
            DO $owned_vip$ DECLARE original content.boards; BEGIN
                PERFORM 1 FROM content.boards WHERE slug='p' FOR UPDATE NOWAIT;
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='p') = {original} THEN RETURN; END IF;
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='p') IS DISTINCT FROM {expected}
                OR EXISTS(SELECT 1 FROM content.posts WHERE board='p')
                OR EXISTS(SELECT 1 FROM content.threads WHERE board='p')
                OR EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE board='p')
                OR EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions WHERE board='p')
                THEN RAISE EXCEPTION 'VIP JPEG cannot restore changed or occupied policy'; END IF;
                SELECT * INTO original FROM jsonb_populate_record(NULL::content.boards,{original});
                UPDATE content.boards SET description=original.description,captcha_enabled=original.captcha_enabled WHERE slug='p';
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='p') IS DISTINCT FROM {original}
                THEN RAISE EXCEPTION 'VIP JPEG did not restore policy'; END IF;
            END $owned_vip$; COMMIT;""")
        self.policy = None

    def issuer_ready(self):
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=1)
        try:
            connection.request('POST', '/v1/vip/ready', b'{}',
                               {'Content-Type': 'application/json', 'Authorization': 'Bearer ' + self.token})
            response = connection.getresponse()
            return response.status == 200 and json.loads(response.read(1024)) == {'ready': True}
        except (OSError, http.client.HTTPException, ValueError):
            return False
        finally:
            connection.close()

    def setup_issuer(self):
        user = account('board-verifier')
        assert user.pw_uid not in (0, self.host._public_identity[0], self.f.intake_user.pw_uid,
                                   self.f.coordinator.pw_uid, self.f.gateway.pw_uid, self.f.http_user.pw_uid)
        assert len({self.key, self.token, self.mail_key}) == 3
        binary = self.f.bin / 'board-verification'
        shutil.copyfile(self.f.binaries / 'board-verification', binary)
        binary.chmod(0o755)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            self.port = listener.getsockname()[1]
        self.issuer = self.f.launch([binary], user, {
            **SAFE, 'VERIFICATION_BIND': f'127.0.0.1:{self.port}',
            'VERIFICATION_DATABASE_URL': os.environ['VERIFICATION_DATABASE_URL'],
            'VERIFICATION_SERVICE_TOKEN': self.token, 'VERIFICATION_KEY': self.key,
            'VERIFICATION_MAIL_KEY': self.mail_key,
        })
        wait_until(self.issuer_ready)
        operation = {'action': 'create', 'identifier': self.identifier,
                     'name': 'Owned Verified JPEG', 'password': self.password}
        result = subprocess.run([self.f.binaries / 'board-vip-admin'],
                                input=json.dumps(operation).encode(), capture_output=True, timeout=15,
                                env={**SAFE, 'MIGRATION_DATABASE_URL': os.environ['MIGRATION_DATABASE_URL'],
                                     'VERIFICATION_KEY': self.key})
        assert result.returncode == 0, 'owned VIP provisioning failed'
        candidate = json.loads(result.stdout)['id']
        assert type(candidate) is int and 0 < candidate < 2**63
        self.account_id = candidate
        path = self.f.root / 'public.env'
        before = path.read_bytes()
        assert b'PUBLIC_VERIFICATION_' not in before and before.endswith(b'\n')
        active = before + f'PUBLIC_VERIFICATION_ADDR=127.0.0.1:{self.port}\nPUBLIC_VERIFICATION_TOKEN={self.token}\n'.encode()
        self.public_env = {'before': before, 'active': active}
        systemctl('stop', self.host.unit.name)
        self.f.write(path, active)
        systemctl('start', self.host.unit.name)
        wait_until(self.host.ready)

    def exercise(self):
        self.lease_board()
        self.setup_issuer()
        user = pwd.getpwuid(REPO.stat().st_uid)
        assert user.pw_uid != 0
        marker = b'Exif\0\0OWNED-VIP-JPEG-METADATA'
        original = (REPO / 'tests/media/fixtures/jpeg/baseline.jpg').read_bytes()
        tagged = original[:2] + b'\xff\xe1' + (len(marker) + 2).to_bytes(2, 'big') + marker + original[2:]
        source = self.f.root / self.filename
        self.f.write(source, tagged, user)
        config = self.f.root / 'owned-vip-jpeg.json'
        # Only the synthetic account option enters the browser. Issuer keys,
        # service bearer tokens and database URLs remain with the supervisor.
        self.f.write(config, json.dumps({'origin': self.host.origin, 'mediaOrigin': f'http://127.0.0.1:{self.f.http_port}', 'source': str(source),
                                        'credential': f'capcode_!{self.identifier}!{self.password}'}), user)
        node = os.environ['PUBLIC_UPLOAD_NODE']
        assert os.path.isabs(node) and os.path.isfile(node)
        self.browser = self.f.launch([node, REPO / 'tests/browser/source-vip-jpeg.mjs', config],
                                     user, {**SAFE, 'HOME': user.pw_dir})
        query = f"SELECT id FROM media.jobs WHERE filename='{self.filename}' AND state='queued';"
        def queued():
            assert self.browser.poll() is None, 'VIP JPEG browser exited before queuing'
            return bool(sql(query))
        wait_until(queued, seconds=25)
        job = sql(query)
        assert HEX.fullmatch(job)
        self.f.ids.append(job)
        assert sql(f"SELECT board||':'||thread_id||':'||capcode::text FROM media.image_context WHERE job_id='{job}'") == 'p:0:true'
        asset = self.f.finish(self.f.dispatch()).decode().strip()
        assert HEX.fullmatch(asset)
        self.f.clean_vm()
        assert marker not in (self.f.objects / f'{asset}.jpg').read_bytes()
        output, error = self.browser.communicate(timeout=45)
        if self.browser.returncode:
            stage = re.search(rb'^OWNED_VIP_JPEG stage=([a-z-]+)(?: line=([0-9]{1,6}):([0-9]{1,6}))?$', error, re.MULTILINE)
            suffix = ' at ' + stage[1].decode() if stage else ''
            raise AssertionError('owned VIP JPEG browser failed' + suffix)
        assert output == b'PASS verified JPEG public form, real isolated processing and metadata stripping\n'
        assert sql(f"SELECT p.capcode||':'||p.staff_authorized_limits::text FROM content.posts p JOIN content.post_media m ON m.post_id=p.id WHERE p.board='p' AND p.thread_id=p.id AND m.job_id='{job}'") == 'verified:false'
        assert sql(f"SELECT source_profile||':'||output_format FROM media.assets WHERE id='{asset}'") == 'still-v6:jpeg'
        print(output.decode().strip(), flush=True)

    def cleanup(self):
        if self.browser is not None:
            self.f.stop(self.browser)
            self.browser = None
        assert re.fullmatch(r'public-vip-jpeg-[0-9a-f]{32}\.jpg', self.filename)
        owned_policy = False
        if self.policy is not None:
            current = json.loads(sql("SELECT to_jsonb(b) FROM content.boards b WHERE slug='p';"))
            assert current in (self.policy['before'], self.policy['active']), 'leased JPEG board changed externally'
            owned_policy = current == self.policy['active']
        jobs = sql(f"SELECT id FROM media.jobs WHERE filename='{self.filename}';").splitlines()
        for job in jobs:
            assert HEX.fullmatch(job)
            if job not in self.f.ids:
                self.f.ids.append(job)
            for post in sql(f"SELECT p.id FROM content.posts p JOIN content.post_media m ON m.post_id=p.id WHERE p.board='p' AND p.thread_id=p.id AND m.job_id='{job}';").splitlines():
                assert re.fullmatch('[1-9][0-9]{0,18}', post)
                sql(f"DELETE FROM content.post_media WHERE post_id={post}; DELETE FROM post_secrets.deletion WHERE post_id={post}; DELETE FROM content.posts WHERE board='p' AND id={post}; DELETE FROM content.threads WHERE board='p' AND id={post};")
        if owned_policy:
            actor = self.host._posting_actor_hex()
            assert re.fullmatch('[0-9a-f]{64}', actor)
            for table in ('posting_history', 'posting_thread_actions'):
                sql(f"DELETE FROM post_secrets.{table} WHERE board='p' AND actor_hash=decode('{actor}','hex');")
        self.restore_board()
        if self.public_env is not None:
            path = self.f.root / 'public.env'
            assert path.read_bytes() in (self.public_env['active'], self.public_env['before']), 'owned public configuration changed'
            systemctl('stop', self.host.unit.name)
            self.f.write(path, self.public_env['before'])
            systemctl('start', self.host.unit.name)
            wait_until(self.host.ready)
            self.public_env = None
        if self.issuer is not None:
            self.f.stop(self.issuer)
            self.issuer = None
        if self.account_id is not None:
            sql(f"BEGIN; SET LOCAL ROLE board_anonymous_owner; DELETE FROM verification_private.vip_accounts WHERE id={self.account_id} AND name='Owned Verified JPEG'; COMMIT;")
            self.account_id = None
