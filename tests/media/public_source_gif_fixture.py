"""One media post on an exclusively leased, disposable source GIF board."""
import json
import os
import pwd
import re
import secrets
from test_dispatch import HEX, REPO, SAFE, sql, wait_until
from source_gif_observation import animation


class PublicSourceGif:
    def __init__(self, host):
        self.host = host
        self.f = host.f
        self.filename = 'public-source-gif-' + secrets.token_hex(16) + '.gif'
        self.browser = None
        self.policy = None

    @staticmethod
    def _json_sql(value):
        return "'" + json.dumps(value, sort_keys=True, separators=(',', ':')).replace("'", "''") + "'::jsonb"

    def lease_board(self):
        assert self.policy is None
        assert re.fullmatch(r'public-source-gif-[0-9a-f]{32}\.gif', self.filename)
        before = json.loads(sql("SELECT to_jsonb(b) FROM content.boards b WHERE slug='gif';"))
        assert before['slug'] == 'gif' and before['staff_only'] is False
        assert before['captcha_enabled'] is True
        active = {**before, 'description': self.filename, 'captcha_enabled': False}
        # Save recovery state before COMMIT; an ambiguous result must remain recoverable.
        self.policy = {'before': before, 'active': active}
        original, expected = self._json_sql(before), self._json_sql(active)
        sql(f"""BEGIN; SET LOCAL standard_conforming_strings=on; SET LOCAL lock_timeout='1s';
            DO $owned_gif$ BEGIN
                PERFORM 1 FROM content.boards WHERE slug='gif' FOR UPDATE NOWAIT;
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='gif') IS DISTINCT FROM {original}
                    OR EXISTS (SELECT 1 FROM content.posts WHERE board='gif')
                    OR EXISTS (SELECT 1 FROM content.threads WHERE board='gif')
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_history WHERE board='gif')
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_thread_actions WHERE board='gif')
                THEN RAISE EXCEPTION 'GIF qualification requires an untouched disposable source board'; END IF;
                UPDATE content.boards SET description='{self.filename}',captcha_enabled=false WHERE slug='gif';
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='gif') IS DISTINCT FROM {expected}
                THEN RAISE EXCEPTION 'GIF qualification changed unrelated policy'; END IF;
            END $owned_gif$; COMMIT;""")

    def restore_board(self):
        if self.policy is None:
            return
        original, expected = self._json_sql(self.policy['before']), self._json_sql(self.policy['active'])
        sql(f"""BEGIN; SET LOCAL standard_conforming_strings=on; SET LOCAL lock_timeout='1s';
            DO $owned_gif$ DECLARE original content.boards; BEGIN
                PERFORM 1 FROM content.boards WHERE slug='gif' FOR UPDATE NOWAIT;
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='gif') = {original} THEN RETURN; END IF;
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='gif') IS DISTINCT FROM {expected}
                    OR EXISTS (SELECT 1 FROM content.posts WHERE board='gif')
                    OR EXISTS (SELECT 1 FROM content.threads WHERE board='gif')
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_history WHERE board='gif')
                    OR EXISTS (SELECT 1 FROM post_secrets.posting_thread_actions WHERE board='gif')
                THEN RAISE EXCEPTION 'GIF qualification cannot restore changed or occupied policy'; END IF;
                SELECT * INTO original FROM jsonb_populate_record(NULL::content.boards,{original});
                UPDATE content.boards SET description=original.description,captcha_enabled=original.captcha_enabled WHERE slug='gif';
                IF (SELECT to_jsonb(b) FROM content.boards b WHERE slug='gif') IS DISTINCT FROM {original}
                THEN RAISE EXCEPTION 'GIF qualification did not restore source policy'; END IF;
            END $owned_gif$; COMMIT;""")
        self.policy = None

    def exercise(self):
        f = self.f
        self.lease_board()
        assert sql("SELECT deletion_known_min_seconds||':'||deletion_unknown_min_seconds FROM content.boards WHERE slug='gif' AND NOT staff_only;") == '60:600'
        user = pwd.getpwuid(REPO.stat().st_uid)
        assert user.pw_uid != 0
        source = f.root / self.filename
        f.write(source, animation(0), user)
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
        # This browser owns a fresh session. Seed only that fixture's network
        # history so the unchanged source policy exercises known-owner deletion.
        # A real new visitor uses the separate 600-second minimum.
        owned_session = f"SELECT a.token_hash FROM post_secrets.anonymous_posts a JOIN content.posts p ON p.id=a.post_id JOIN content.post_media m ON m.post_id=p.id WHERE p.board='gif' AND p.thread_id=p.id AND m.job_id='{job}'"
        wait_until(lambda: sql(f"SELECT count(*) FROM ({owned_session}) owned;") == '1', seconds=25)
        assert sql(f"WITH changed AS (UPDATE post_secrets.anonymous_sessions s SET created_at=LEAST(s.created_at,extract(epoch FROM now())::bigint-901),network_at=extract(epoch FROM now())::bigint-901 WHERE s.token_hash IN ({owned_session}) RETURNING 1) SELECT count(*) FROM changed;") == '1'
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
        self.restore_board()
