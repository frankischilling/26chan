"""One source-board post, without changing or deleting the imported board."""
import os
import pwd
import re
import secrets
from test_dispatch import HEX, REPO, SAFE, sql, wait_until


class PublicSourceGif:
    def __init__(self, host):
        self.host = host
        self.f = host.f
        self.filename = 'public-source-gif-' + secrets.token_hex(16) + '.gif'
        self.browser = None

    def exercise(self):
        f = self.f
        assert sql("SELECT deletion_known_min_seconds FROM content.boards WHERE slug='gif' AND NOT staff_only;") == '60'
        user = pwd.getpwuid(REPO.stat().st_uid)
        assert user.pw_uid != 0
        source = f.root / self.filename
        f.write(source, (REPO / 'tests/media/fixtures/gif/animated.gif').read_bytes(), user)
        node = os.environ['PUBLIC_UPLOAD_NODE']
        assert os.path.isabs(node) and os.path.isfile(node)
        self.browser = f.launch([node, REPO / 'tests/browser/source-gif-upload.mjs', self.host.origin, source],
                                user, {**SAFE, 'HOME': user.pw_dir})
        query = f"SELECT id FROM media.jobs WHERE filename='{self.filename}' AND state='queued';"
        def queued():
            assert self.browser.poll() is None, 'source GIF browser exited before queueing'
            return bool(sql(query))
        wait_until(queued, seconds=25)
        job = sql(query)
        assert HEX.fullmatch(job)
        f.ids.append(job)
        asset = f.finish(f.dispatch()).decode().strip()
        assert HEX.fullmatch(asset)
        f.clean_vm()
        output, error = self.browser.communicate(timeout=110)
        if self.browser.returncode:
            location = re.search(rb'/source-gif-upload.mjs:([0-9]{1,6}):([0-9]{1,6})', error)
            suffix = ':' + ':'.join(v.decode() for v in location.groups()) if location else ''
            raise AssertionError('owned source GIF browser failed' + suffix)
        assert output.startswith(b'PASS source GIF real public posting')
        assert sql(f"SELECT count(*) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board='gif' AND m.asset_id='{asset}' AND m.file_deleted AND NOT p.deleted;") == '1'
        assert sql(f"SELECT source_profile||':'||thumbnail_format FROM media.assets WHERE id='{asset}'") == 'gif-v7:jpeg'
        cleaned = f.finish(f.launch([f.bin / 'media-publish', 'reconcile', f.objects], env=f.writer))
        assert int(cleaned.strip()) >= 1
        assert not (f.objects / f'{asset}.gif').exists()
        assert not (f.objects / f'{asset}.thumb.jpg').exists()
        print(output.decode().strip(), flush=True)

    def cleanup(self):
        if self.browser is not None:
            self.f.stop(self.browser)
        assert re.fullmatch(r'public-source-gif-[0-9a-f]{32}\.gif', self.filename)
        jobs = sql(f"SELECT id FROM media.jobs WHERE filename='{self.filename}';").splitlines()
        for job in jobs:
            assert HEX.fullmatch(job)
            if job not in self.f.ids:
                self.f.ids.append(job)
            for post in sql(f"SELECT p.id FROM content.posts p JOIN content.post_media m ON m.post_id=p.id WHERE p.board='gif' AND p.thread_id=p.id AND m.job_id='{job}';").splitlines():
                assert re.fullmatch('[1-9][0-9]{0,18}', post)
                sql(f"DELETE FROM content.post_media WHERE post_id={post}; DELETE FROM post_secrets.deletion WHERE post_id={post}; DELETE FROM content.posts WHERE board='gif' AND id={post}; DELETE FROM content.threads WHERE board='gif' AND id={post};")
        actor = self.host._posting_actor_hex()
        assert re.fullmatch('[0-9a-f]{64}', actor)
        for table in ('posting_history', 'posting_thread_actions'):
            sql(f"DELETE FROM post_secrets.{table} WHERE board='gif' AND actor_hash=decode('{actor}','hex');")
