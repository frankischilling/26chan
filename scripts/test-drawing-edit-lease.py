#!/usr/bin/env python3
"""Exercise the actual /i/ drawing fixture lease against disposable PostgreSQL.

The calling test runner must supply a migrated, disposable database through
MIGRATION_DATABASE_URL and explicitly opt in with DRAWING_EDIT_LEASE_DISPOSABLE=1.
No browser, media dispatcher, live credentials, or PHP application is started.
"""

import hashlib
import json
import os
import pathlib
import re
import secrets
import subprocess
import sys
import tempfile
import urllib.parse
from types import SimpleNamespace
from unittest import mock


REPO = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO / 'tests' / 'media'))
import public_drawing_fixture  # noqa: E402
from public_drawing_fixture import PublicDrawingEdit  # noqa: E402
from test_dispatch import PG, SAFE  # noqa: E402


def must(condition, description):
    if not condition:
        raise AssertionError(description)


def statement(sql_text):
    """Use the same real migrator/psql connection as the VM fixture."""
    return guarded_sql(None)(sql_text)


def board_policy():
    result = statement("SELECT to_jsonb(b) FROM content.boards b WHERE slug='i';")
    must(result.startswith('{'), 'source /i/ board is absent')
    return json.loads(result)


def source_rows():
    """Full JSON snapshots for refusal/rollback and final restoration checks."""
    query = """SELECT jsonb_build_object(
        'board',(SELECT to_jsonb(b) FROM content.boards b WHERE slug='i'),
        'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY t.id),'[]'::jsonb)
            FROM content.threads t WHERE t.board='i'),
        'posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY p.id),'[]'::jsonb)
            FROM content.posts p WHERE p.board='i'),
        'media',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY m.post_id),'[]'::jsonb)
            FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board='i'),
        'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY h.post_id),'[]'::jsonb)
            FROM post_secrets.posting_history h WHERE h.board='i'),
        'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.actor_hash),'[]'::jsonb)
            FROM post_secrets.posting_thread_actions a WHERE a.board='i'),
        'deletion',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY d.post_id),'[]'::jsonb)
            FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id WHERE p.board='i'),
        'reports',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY r.id),'[]'::jsonb)
            FROM content.reports r WHERE r.board='i'));"""
    return json.loads(statement(query))


def outside_rows():
    """Compare untouched foreign rows without ever fetching or printing them."""
    return statement("""SELECT md5(coalesce(string_agg(row_data, E'\\n' ORDER BY kind, id),''))
        FROM (
            SELECT 'boards' AS kind,slug AS id,to_jsonb(b)::text AS row_data
                FROM content.boards b WHERE slug<>'i'
            UNION ALL SELECT 'threads',id::text,to_jsonb(t)::text
                FROM content.threads t WHERE board<>'i'
            UNION ALL SELECT 'posts',id::text,to_jsonb(p)::text
                FROM content.posts p WHERE board<>'i'
            UNION ALL SELECT 'media',m.post_id::text,to_jsonb(m)::text
                FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board<>'i'
            UNION ALL SELECT 'history',post_id::text,to_jsonb(h)::text
                FROM post_secrets.posting_history h WHERE board<>'i'
            UNION ALL SELECT 'actions',encode(actor_hash,'hex')||board,to_jsonb(a)::text
                FROM post_secrets.posting_thread_actions a WHERE board<>'i'
            UNION ALL SELECT 'reports',id::text,to_jsonb(r)::text
                FROM content.reports r WHERE board<>'i'
        ) unchanged;""")


def assert_empty_i(baseline):
    now = source_rows()
    must(now == baseline, 'source board has not been restored byte-for-byte at the row level')


class ExpectedLeaseRefusal(AssertionError):
    """The selected guard raised its expected PostgreSQL exception."""


def guarded_sql(expected_reason):
    """Read SQLSTATE and the exact expected guard privately; never emit stderr."""
    url = urllib.parse.urlparse(os.environ['MIGRATION_DATABASE_URL'])
    pg_env = dict(PGHOST=url.hostname, PGPORT=str(url.port or 5432),
                  PGUSER=urllib.parse.unquote(url.username),
                  PGPASSWORD=urllib.parse.unquote(url.password),
                  PGDATABASE=urllib.parse.unquote(url.path.lstrip('/')))

    def execute(statement_text):
        result = subprocess.run([PG, '-XAt', '-v', 'ON_ERROR_STOP=1', '-v', 'VERBOSITY=verbose'],
                                input=statement_text, text=True, capture_output=True,
                                timeout=10, env={**SAFE, **pg_env})
        if result.returncode == 0:
            return result.stdout.strip()
        code = re.search(r'\bERROR:\s*([A-Z0-9]{5}):', result.stderr)
        primary = re.search(r'\bERROR:\s*[A-Z0-9]{5}:\s*([^\r\n]+)', result.stderr)
        reason = primary.group(1) if primary else ''
        if (expected_reason is not None and result.returncode == 3
                and code and code.group(1) == 'P0001'
                and reason == expected_reason):
            raise ExpectedLeaseRefusal(expected_reason)
        classification = code.group(1) if code else 'unclassified'
        identifiers = []
        # A fixed migration guard is useful evidence; never print arbitrary
        # PostgreSQL messages, failing-row detail, credentials or submitted text.
        known_guards = {
            message for migration in (REPO / 'migrations').glob('*.sql')
            for message in re.findall(r"RAISE EXCEPTION '([^']+)'", migration.read_text())
            if '%' not in message
        }
        if reason in known_guards:
            identifiers.append('guard=' + reason)
        for field in ('TABLE NAME', 'COLUMN NAME', 'CONSTRAINT NAME'):
            match = re.search(r'^' + field + r':\s*([a-z_][a-z0-9_]*)\s*$', result.stderr, re.MULTILINE)
            if match:
                identifiers.append(field.lower() + '=' + match.group(1))
        raise AssertionError('unexpected PostgreSQL SQLSTATE while qualifying drawing lease: '
                             + classification + ('; ' + ', '.join(identifiers) if identifiers else ''))

    return execute


def refused(label, callback, expected_reason):
    previous = source_rows()
    with mock.patch.object(public_drawing_fixture, 'sql', side_effect=guarded_sql(expected_reason)):
        try:
            callback()
        except ExpectedLeaseRefusal:
            pass
        else:
            raise AssertionError(label + ' incorrectly committed')
    must(source_rows() == previous, label + ' failed to roll back all rows')
    print('PASS ' + label + ': real PostgreSQL refusal and rollback', flush=True)


class ObservedFalsePredicate(str):
    def __init__(self, value):
        self.comparisons = []

    def __eq__(self, expected):
        if expected == 't':
            caller = sys._getframe(1)
            self.comparisons.append((caller.f_code, caller.f_lineno))
        return super().__eq__(expected)


def rejected_predicate(label, callback, forged_fragment, control_fragment, query_marker):
    """Prove the intended forged SQL predicate is false and its valid control true."""
    before = source_rows()
    actual_sql = public_drawing_fixture.sql
    observed = 0
    false_result = None

    def checked_sql(query):
        nonlocal observed, false_result
        observed += 1
        must(observed == 1 and query.startswith('SELECT (')
             and query.count(forged_fragment) == 1 and query_marker in query,
             label + ' did not query its intended ownership predicate')
        control = query.replace(forged_fragment, control_fragment, 1)
        must(actual_sql(control) == 't', label + ' valid ownership control was not true')
        must(actual_sql(query) == 'f', label + ' forged ownership predicate did not return f')
        # This behaves like the actual 'f'. Record where the fixture compares
        # it with 't', but let the fixture's own assert reject that false value.
        false_result = ObservedFalsePredicate('f')
        return false_result

    with mock.patch.object(public_drawing_fixture, 'sql', side_effect=checked_sql):
        try:
            callback()
        except AssertionError as failure:
            must(observed == 1 and false_result is not None
                 and len(false_result.comparisons) == 1,
                 label + ' failed without comparing the SQL result to true')
            comparison_code, comparison_line = false_result.comparisons[0]
            trace = failure.__traceback__
            while trace is not None and trace.tb_next is not None:
                trace = trace.tb_next
            must(type(failure) is AssertionError and not failure.args
                 and comparison_code in (
                     public_drawing_fixture.PublicDrawingEdit.register_owner.__code__,
                     public_drawing_fixture.PublicDrawingUpload.register_receipt.__code__,
                 )
                 and trace is not None
                 and trace.tb_frame.f_code is comparison_code
                 and trace.tb_lineno == comparison_line,
                 label + ' failed at an unrelated assertion')
        else:
            raise AssertionError(label + ' accepted forged ownership')
    must(observed == 1, label + ' skipped its SQL ownership predicate')
    must(source_rows() == before, label + ' changed persisted rows')


def rejected_wrong_target(label, fixture, job, capability_hash, owner_thread):
    """A mismatched Edit target is a local guard, before receipt SQL/delegation."""
    before = source_rows()
    receipts = dict(fixture.receipts)
    ids = list(fixture.f.ids)
    must(fixture.mode == 'image-edit' and fixture.lease_phase == 'active'
         and fixture.owner_thread == owner_thread and owner_thread != '0'
         and job not in receipts and re.fullmatch('[a-f0-9]{32}', job)
         and re.fullmatch('[a-f0-9]{64}', capability_hash),
         label + ' did not isolate the wrong-target precondition')
    with mock.patch.object(public_drawing_fixture.PublicDrawingUpload, 'register_receipt',
                           side_effect=RuntimeError('Unexpected receipt delegation')) as delegated:
        try:
            fixture.register_receipt(fixture.marker, job, capability_hash, '0', owner_thread)
        except AssertionError as failure:
            trace = failure.__traceback__
            must(type(failure) is AssertionError and not failure.args
                 and trace is not None and trace.tb_next is not None
                 and trace.tb_next.tb_frame.f_code is
                 public_drawing_fixture.PublicDrawingEdit.register_receipt.__code__
                 and trace.tb_next.tb_next is None,
                 label + ' failed at an unrelated local assertion')
        else:
            raise AssertionError(label + ' accepted the wrong target')
        delegated.assert_not_called()
    must(fixture.receipts == receipts and fixture.f.ids == ids,
         label + ' registered local receipt state')
    must(source_rows() == before, label + ' changed persisted rows')


def rolled_back_false(label, sql_text):
    """Extract the sole SELECT result from psql command tags, then check rollback."""
    before = source_rows()
    transcript = statement(sql_text).splitlines()
    must(transcript.count('f') == 1 and 't' not in transcript,
         label + ' incorrectly accepted its temporary mutation')
    must(source_rows() == before, label + ' left a committed mutation')


def actor_context(actor):
    must(len(actor) == 64 and all(ch in '0123456789abcdef' for ch in actor),
         'unsafe synthetic actor identifier')
    return f"SELECT set_config('board.posting_actor','{actor}',true);"


def formatting_context(comment, op):
    """Use the real formatter for synthetic rows on the unchanged source board."""
    must(re.fullmatch(r'[A-Za-z0-9 /-]{1,256}', comment) is not None,
         'drawing fixture comment is not bounded synthetic text')
    policy = board_policy()
    request = {name: policy[name] for name in (
        'word_filter_enabled', 'word_filter_profile', 'comment_spoiler_cleanup',
        'comment_code_spacing', 'comment_sjis_spacing', 'op_markup')}
    request.update(comment=comment, op=op)
    formatter = pathlib.Path(os.environ.get(
        'DRAWING_LEASE_FORMATTER', str(REPO / 'target/debug/examples/drawing-lease-format'))).resolve(strict=True)
    mode = formatter.stat()
    must(formatter.name == 'drawing-lease-format' and formatter.is_file()
         and mode.st_uid in (0, os.getuid()) and mode.st_mode & 0o022 == 0,
         'drawing formatter is not an owned executable')
    result = subprocess.run([str(formatter)], input=json.dumps(request), text=True,
                            capture_output=True, timeout=10, env=SAFE)
    must(result.returncode == 0, 'drawing fixture formatter rejected its synthetic input')
    payload = result.stdout.strip()
    must(re.fullmatch(r'[0-9a-f]{22,262144}', payload) is not None and len(payload) % 2 == 0,
         'drawing fixture formatter returned invalid typed data')
    # The formatter proves both its saved and visible text equal this bounded
    # synthetic comment. Neither formatter process nor stdout carries a credential.
    return (f"SELECT set_config('board.wordfilter_payload','{payload}',true);"
            f"SELECT set_config('board.wordfilter_search','{comment}',true);")


def number():
    value = statement("SELECT nextval('content.post_number');")
    must(value.isascii() and value.isdecimal() and 0 < int(value) <= 9223372036854775807,
         'unusable synthetic post identifier')
    return int(value)


class Qualification:
    def __init__(self):
        self.root = tempfile.TemporaryDirectory(prefix='26chan-dispatch-')
        path = pathlib.Path(self.root.name).resolve()
        must(path.name.startswith('26chan-dispatch-') and len(path.name) == len('26chan-dispatch-') + 8,
             'unexpected private witness directory')
        quarantine = path / 'quarantine'
        quarantine.mkdir(mode=0o700)
        quarantine.chmod(0o700)
        self.f = SimpleNamespace(root=path, quarantine=quarantine,
                                 intake_user=SimpleNamespace(pw_uid=os.getuid()), ids=[])
        self.host = SimpleNamespace(f=self.f, origin='http://127.0.0.1:1',
                                    _poster_id_key=secrets.token_hex(32))
        self.fixture = None
        self.jobs = []  # (job,asset,capability_hash,lease_token), exclusively synthetic
        self.synthetic_posts = {}  # post_id -> (thread_id,comment,actor)
        self.synthetic_threads = set()

    def new_fixture(self):
        fixture = PublicDrawingEdit(self.host)
        self.fixture = fixture
        return fixture

    def add_thread(self, thread_id, actor, comment):
        must(thread_id not in self.synthetic_threads, 'thread identifier reused')
        statement(f"""BEGIN; {actor_context(actor)} {formatting_context(comment, True)}
            INSERT INTO content.threads(id,board) VALUES({thread_id},'i');
            INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
                VALUES({thread_id},'i',{thread_id},'Synthetic','','{comment}');
            COMMIT;""")
        self.synthetic_threads.add(thread_id)
        self.synthetic_posts[thread_id] = (thread_id, comment, actor)

    def add_reply(self, post_id, owner, actor, comment, drawing_time=None, source=None):
        must(post_id not in self.synthetic_posts, 'post identifier reused')
        settings = actor_context(actor) + formatting_context(comment, False)
        if drawing_time is not None:
            must(str(drawing_time).isascii() and str(drawing_time).isdecimal(),
                 'invalid synthetic drawing time')
            settings += f"SELECT set_config('board.drawing_time_seconds','{drawing_time}',true);"
            if source is not None:
                must(str(source).isascii() and str(source).isdecimal(),
                     'invalid synthetic source identifier')
                settings += f"SELECT set_config('board.drawing_source_post_id','{source}',true);"
        statement(f"""BEGIN; {settings}
            INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
                VALUES({post_id},'i',{owner},'Synthetic','','{comment}');
            COMMIT;""")
        self.synthetic_posts[post_id] = (owner, comment, actor)

    def queue_job(self):
        job, asset, lease, capability_hash = (secrets.token_hex(16), secrets.token_hex(16),
                                               secrets.token_hex(16), secrets.token_hex(32))
        self.jobs.append((job, asset, capability_hash, lease))
        statement(f"""BEGIN;
            INSERT INTO media.jobs(id,filename,state,input_bytes,expires_at)
                VALUES('{job}','tegaki.png','queued',64,clock_timestamp()+interval '1 hour');
            INSERT INTO media_intake.handles(job_id,capability_hash)
                VALUES('{job}',decode('{capability_hash}','hex'));
            COMMIT;""")
        witness = self.f.quarantine / (job + '.input')
        witness.write_bytes(bytes(64))
        witness.chmod(0o600)
        return job, asset, capability_hash

    def approve_job(self, job):
        matching = [entry for entry in self.jobs if entry[0] == job]
        must(len(matching) == 1, 'synthetic media identity is ambiguous')
        _, asset, _, lease = matching[0]
        sha = hashlib.sha256(('drawing-edit-lease-' + job).encode()).hexdigest()
        statement(f"""BEGIN;
            UPDATE media.jobs SET state='published',attempts=1,lease_token='{lease}',expires_at=NULL,
                output_sha256='{sha}',output_bytes=100 WHERE id='{job}'
                AND state='queued' AND attempts=0 AND lease_token IS NULL;
            INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at)
                VALUES('{asset}','{job}','{lease}','{sha}',100,400,400,'approved',clock_timestamp());
            COMMIT;""")
        must(statement(f"SELECT id FROM media.assets WHERE job_id='{job}' AND state='approved';") == asset,
             'synthetic approval was not persisted')
        must(statement(f"SELECT state FROM media.jobs WHERE id='{job}' AND lease_token='{lease}' AND attempts=1;") == 'published',
             'synthetic published job was not persisted')

    def attach(self, post_id, job, *, time=None, source=None):
        matching = [entry for entry in self.jobs if entry[0] == job]
        must(len(matching) == 1, 'synthetic job is not owned')
        asset = matching[0][1]
        settings = ''
        if time is not None:
            settings += f"SELECT set_config('board.drawing_time_seconds','{time}',true);"
            if source is not None:
                settings += f"SELECT set_config('board.drawing_source_post_id','{source}',true);"
        statement(f"""BEGIN; {settings}
            INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler)
                VALUES({post_id},'{job}','{asset}','tegaki.png',100,400,400,false);
            COMMIT;""")
        must(statement(f"SELECT job_id FROM content.post_media WHERE post_id={post_id};") == job,
             'synthetic attachment was not persisted')

    def remove_post(self, post_id):
        """Remove only an exact inserted post with its original actor and media."""
        expected = self.synthetic_posts.get(post_id)
        must(expected is not None, 'refusing to remove unregistered post')
        owner, comment, actor = expected
        entries = [entry for entry in self.jobs]
        allowed = ','.join("('" + job + "','" + asset + "')" for job, asset, _, _ in entries)
        allowed_check = f'(m.job_id,m.asset_id) NOT IN (VALUES {allowed})' if allowed else 'true'
        statement(f"""BEGIN;
            DO $owned$ BEGIN
                IF NOT EXISTS (SELECT 1 FROM content.posts p
                    JOIN post_secrets.posting_history h ON h.post_id=p.id AND h.board=p.board
                    AND h.thread_id=p.thread_id
                    WHERE p.id={post_id} AND p.board='i' AND p.thread_id={owner}
                      AND p.comment='{comment}' AND NOT p.deleted
                      AND h.actor_hash=decode('{actor}','hex'))
                    OR EXISTS (SELECT 1 FROM content.post_media m WHERE m.post_id={post_id}
                        AND {allowed_check})
                THEN RAISE EXCEPTION 'Synthetic post evidence changed'; END IF;
                DELETE FROM content.post_media WHERE post_id={post_id};
                DELETE FROM post_secrets.deletion WHERE post_id={post_id};
                DELETE FROM content.posts WHERE board='i' AND id={post_id};
            END $owned$;
            COMMIT;""")
        del self.synthetic_posts[post_id]

    def remove_thread(self, thread_id, actor=None):
        must(thread_id in self.synthetic_threads and thread_id not in self.synthetic_posts,
             'synthetic thread still has posts or is not owned')
        statement(f"""BEGIN;
            DO $owned$ BEGIN
                IF NOT EXISTS (SELECT 1 FROM content.threads WHERE board='i' AND id={thread_id})
                    OR EXISTS (SELECT 1 FROM content.posts WHERE board='i' AND thread_id={thread_id})
                THEN RAISE EXCEPTION 'Synthetic thread evidence changed'; END IF;
                DELETE FROM content.threads WHERE board='i' AND id={thread_id};
            END $owned$; COMMIT;""")
        self.synthetic_threads.remove(thread_id)
        if actor is not None:
            statement(f"DELETE FROM post_secrets.posting_thread_actions WHERE board='i' AND actor_hash=decode('{actor}','hex');")

    def clear_jobs(self):
        for job, asset, _, lease in self.jobs:
            statement(f"""BEGIN;
                DO $owned$ BEGIN
                    IF EXISTS (SELECT 1 FROM content.post_media WHERE job_id='{job}')
                    THEN RAISE EXCEPTION 'Synthetic asset remains attached'; END IF;
                    IF EXISTS (SELECT 1 FROM media.assets WHERE id='{asset}' AND
                        (job_id<>'{job}' OR lease_token<>'{lease}' OR width<>400 OR height<>400))
                    THEN RAISE EXCEPTION 'Synthetic asset evidence changed'; END IF;
                    DELETE FROM media.assets WHERE id='{asset}' AND job_id='{job}' AND lease_token='{lease}';
                    DELETE FROM media_intake.handles WHERE job_id='{job}';
                    DELETE FROM media.jobs WHERE id='{job}' AND filename='tegaki.png';
                    IF EXISTS (SELECT 1 FROM media.jobs WHERE id='{job}')
                       OR EXISTS (SELECT 1 FROM media.assets WHERE id='{asset}')
                    THEN RAISE EXCEPTION 'Synthetic media cleanup incomplete'; END IF;
                END $owned$; COMMIT;""")
        self.jobs.clear()

    def close(self):
        self.root.cleanup()


def preflight():
    must(os.environ.get('DRAWING_EDIT_LEASE_DISPOSABLE') == '1',
         'explicit DRAWING_EDIT_LEASE_DISPOSABLE=1 required for a disposable database')
    must(bool(os.environ.get('MIGRATION_DATABASE_URL')),
         'migrator URL for the root-provided disposable database is missing')
    must(statement("SELECT current_user || '|' || current_setting('is_superuser');") == 'board_migrator|off',
         'qualification requires the constrained board_migrator role')
    must(statement("SELECT to_regclass('content.post_media') IS NOT NULL AND "
                   "to_regclass('post_secrets.posting_history') IS NOT NULL AND "
                   "to_regprocedure('content.stamp_drawing_annotation()') IS NOT NULL;") == 't',
         'drawing schema/migration 0126 is missing')
    must(statement("SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing');") == '0',
         'run against an idle disposable queue')
    must(statement("""SELECT (SELECT count(*) FROM content.posts WHERE board='i')=0 AND
        (SELECT count(*) FROM content.threads WHERE board='i')=0 AND
        (SELECT count(*) FROM post_secrets.posting_history WHERE board='i')=0 AND
        (SELECT count(*) FROM post_secrets.posting_thread_actions WHERE board='i')=0 AND
        (SELECT count(*) FROM content.reports WHERE board='i')=0;""") == 't',
         'source /i/ contains existing content or private admission records')
    policy = board_policy()
    must(policy['title'] == 'Oekaki' and policy['slug'] == 'i' and policy['oekaki']
         and policy['oekaki_replays'] and policy['oekaki_width'] == policy['oekaki_height'] == 400
         and policy['image_limit'] > 0 and not policy['staff_only'] and not policy['text_only'],
         'source /i/ policy does not support the owned edit lease')
    must(statement("""SELECT NOT has_column_privilege('board_public','content.posts',
            'drawing_time_seconds','INSERT')
        AND NOT has_column_privilege('board_public','content.posts','drawing_time_seconds','UPDATE')
        AND NOT has_column_privilege('board_public','content.posts','drawing_source_post_id','INSERT')
        AND NOT has_column_privilege('board_public','content.posts','drawing_source_post_id','UPDATE')
        AND NOT has_table_privilege('board_public','content.post_media','INSERT')
        AND has_column_privilege('board_attachment_owner','content.posts','drawing_time_seconds','UPDATE')
        AND (SELECT pg_get_userbyid(proowner)='board_attachment_owner' AND prosecdef
            FROM pg_proc WHERE oid='content.stamp_drawing_annotation()'::regprocedure);""") == 't',
         'drawing stamp role, ACL or SECURITY DEFINER boundary is unsafe')
    print('PASS drawing lease prerequisites: disposable migrator, empty /i/, typed writer ACL', flush=True)


def qualify():
    preflight()
    baseline = source_rows()
    outside = outside_rows()
    test = Qualification()
    fixture = None
    foreign = secrets.token_hex(32)
    try:
        # The board snapshot is taken before mutation, and any existing post
        # must veto the lease without changing public policy or private data.
        foreign_thread = number()
        test.add_thread(foreign_thread, foreign, 'Foreign preexisting post')
        fixture = test.new_fixture()
        refused('preexisting /i/ post', fixture.create_board,
                'Drawing Edit requires an untouched disposable source board')
        must(fixture.lease_phase == 'prepared', 'lease recovery was not pre-armed')
        fixture.cleanup_board()
        test.remove_post(foreign_thread)
        test.remove_thread(foreign_thread, foreign)
        assert_empty_i(baseline)

        # Empty content is insufficient: an existing private thread action must
        # also be protected from claiming another actor's prior ownership.
        statement(f"""INSERT INTO post_secrets.posting_thread_actions(actor_hash,board,request_at)
            VALUES(decode('{foreign}','hex'),'i',0);""")
        fixture = test.new_fixture()
        refused('preexisting private posting action', fixture.create_board,
                'Drawing Edit requires an untouched disposable source board')
        fixture.cleanup_board()
        statement(f"DELETE FROM post_secrets.posting_thread_actions WHERE board='i' AND actor_hash=decode('{foreign}','hex') AND request_at=0;")
        assert_empty_i(baseline)

        # Emulate an interrupted client transport after PostgreSQL committed.
        # The actual SQL still executes; only its success acknowledgement is lost.
        fixture = test.new_fixture()
        original_sql = public_drawing_fixture.sql
        def committed_without_reply(command):
            result = original_sql(command)
            if "UPDATE content.boards SET title='Drawing qualification'" in command:
                raise AssertionError('transport lost after COMMIT')
            return result
        with mock.patch.object(public_drawing_fixture, 'sql', side_effect=committed_without_reply):
            try:
                fixture.create_board()
            except AssertionError as failure:
                must(str(failure) == 'transport lost after COMMIT', 'ambiguous transport simulation failed')
            else:
                raise AssertionError('ambiguous transport simulation unexpectedly returned')
        must(fixture.lease_phase == 'prepared' and fixture.created,
             'lease recovery no longer knows how to clean an uncertain COMMIT')
        must(board_policy() == fixture.lease['active'], 'committed lease was not observed')
        fixture.cleanup_board()
        assert_empty_i(baseline)
        print('PASS ambiguous COMMIT: pre-saved policy permits exact restoration', flush=True)

        fixture = test.new_fixture()
        fixture.create_board()
        must(fixture.lease_phase == 'active' and board_policy() == fixture.lease['active'],
             'leased policy differs from exact saved active row')
        must(outside_rows() == outside, 'lease changed an unrelated board or content')

        # Corrupting any operator policy must reject cleanup with no partial
        # post deletion or partial restoration, then restore only our mutation.
        statement("UPDATE content.boards SET description='foreign-policy' WHERE slug='i';")
        refused('foreign board policy', fixture.cleanup_board,
                'Drawing Edit policy ownership changed')
        statement(f"UPDATE content.boards SET description='{fixture.marker}' WHERE slug='i' AND description='foreign-policy';")
        must(board_policy() == fixture.lease['active'], 'test policy repair was incomplete')

        owner = number()
        owner_str = str(owner)
        actor = fixture._posting_actor_hex()
        test.add_thread(owner, actor, 'Drawing ownership ' + fixture.marker)
        rejected_predicate(
            'unowned OWNER number',
            lambda: fixture.register_owner(fixture.marker, str(owner + 1)),
            f'p.id={owner + 1} AND p.thread_id=p.id',
            f'p.id={owner} AND p.thread_id=p.id',
            "AND (SELECT count(*) FROM content.posts WHERE board='i')=1")
        fixture.register_owner(fixture.marker, owner_str)
        must(fixture.owner_thread == owner_str, 'OWNER did not bind the owned thread')

        job1, asset1, hash1 = test.queue_job()
        rejected_wrong_target('receipt with wrong OWNER target', fixture, job1, hash1, owner_str)
        forged_hash = secrets.token_hex(32)
        must(forged_hash != hash1, 'forged receipt hash coincided with its valid control')
        rejected_predicate(
            'receipt with forged capability hash',
            lambda: fixture.register_receipt(fixture.marker, job1, forged_hash,
                                             owner_str, owner_str),
            f"h.capability_hash=decode('{forged_hash}','hex')",
            f"h.capability_hash=decode('{hash1}','hex')",
            'JOIN media_intake.handles h ON h.job_id=j.id')
        must(not fixture.receipts and not test.f.ids, 'forged receipt became registered')
        fixture.register_receipt(fixture.marker, job1, hash1, owner_str, owner_str)
        must(fixture.receipts[job1]['owner_thread'] == owner_str and job1 in test.f.ids,
             'first receipt was not bound to OWNER')
        test.approve_job(job1)
        first = number()
        first_comment = 'Drawing qualification ' + fixture.marker
        test.add_reply(first, owner, actor, first_comment)
        test.attach(first, job1, time='90', source=owner_str)
        must(statement(f"SELECT drawing_time_seconds::text || '|' || "
                       f"coalesce(drawing_source_post_id::text,'NULL') FROM content.posts WHERE id={first};")
             == '90|NULL', 'invalid source did not fall back to time-only metadata')
        must(statement(f"SELECT comment FROM content.posts WHERE id={first};") == first_comment,
             'drawing metadata changed the stored post comment')
        fixture.approved.append((job1, asset1))

        job2, asset2, hash2 = test.queue_job()
        fixture.register_receipt(fixture.marker, job2, hash2, owner_str, owner_str)
        must(len(fixture.receipts) == 2 and len(fixture.approved) == 1,
             'second drawing source proof has the wrong receipt cardinality')
        proof = fixture.approval_source_sql(fixture.receipts[job2])
        must(statement('SELECT ' + proof + ';') == 't',
             'second approval cannot prove the first owned, live image')
        rolled_back_false('file-deleted source image', f"""BEGIN;
            UPDATE content.post_media SET file_deleted=true WHERE post_id={first};
            SELECT {proof}; ROLLBACK;""")
        rolled_back_false('wrong-sized source image', f"""BEGIN;
            UPDATE content.post_media SET width=399 WHERE post_id={first};
            SELECT {proof}; ROLLBACK;""")
        rolled_back_false('foreign source actor', f"""BEGIN;
            UPDATE post_secrets.posting_history SET actor_hash=decode('{foreign}','hex') WHERE post_id={first};
            SELECT {proof}; ROLLBACK;""")
        test.approve_job(job2)
        second = number()
        second_comment = 'Drawing edit qualification ' + fixture.marker
        test.add_reply(second, owner, actor, second_comment)
        test.attach(second, job2, time='95', source=str(first))
        must(statement(f"SELECT drawing_time_seconds::text || '|' || "
                       f"coalesce(drawing_source_post_id::text,'NULL') FROM content.posts WHERE id={second};")
             == f'95|{first}', 'eligible source was not stamped as typed metadata')
        must(statement(f"SELECT comment FROM content.posts WHERE id={second};") == second_comment,
             'second annotation was injected into comment HTML')
        fixture.approved.append((job2, asset2))
        must(len(fixture.approved) == 2, 'approved image edit count changed')
        statement(f"UPDATE content.post_media SET file_deleted=true WHERE post_id IN ({first},{second});")
        must(statement(f"SELECT drawing_source_post_id FROM content.posts WHERE id={second};") == str(first),
             'deleted-file source metadata lost its reference')
        print('PASS OWNER/receipts: approved PNG source, no replay, typed time and source links', flush=True)

        # Exercise the tombstone branch independently of the real reconciler.
        # The browser/VM suite records this witness only after verified output
        # and approval removal; this synthetic case qualifies the SQL guard.
        statement(f"DELETE FROM media.assets WHERE id='{asset1}' AND job_id='{job1}';")
        must(statement(f"SELECT NOT EXISTS (SELECT 1 FROM media.assets WHERE id='{asset1}');") == 't',
             'synthetic retired approval remains')
        refused('unwitnessed retired approval', fixture.cleanup_board,
                'Drawing Edit found unowned posts or attachments')
        fixture.reconciled.add((job1, asset1))
        statement(f"UPDATE content.post_media SET file_deleted=false WHERE post_id={first};")
        refused('retired approval on a live attachment', fixture.cleanup_board,
                'Drawing Edit found unowned posts or attachments')
        statement(f"UPDATE content.post_media SET file_deleted=true WHERE post_id={first};")
        print('PASS retired approval requires an exact reconciliation witness and a deleted-file tombstone', flush=True)

        # A deliberately unapproved text reply, extra thread, extra media, a
        # foreign posting actor and a live report must all veto cleanup. The
        # full /i/ snapshot makes partial DELETE/UPDATE visible after refusal.
        extra_text = number()
        test.add_reply(extra_text, owner, actor, 'Synthetic foreign text')
        refused('spurious text post', fixture.cleanup_board,
                'Drawing Edit found unowned posts or attachments')
        test.remove_post(extra_text)

        extra_thread = number()
        statement(f"INSERT INTO content.threads(id,board) VALUES({extra_thread},'i');")
        test.synthetic_threads.add(extra_thread)
        refused('spurious thread', fixture.cleanup_board,
                'Drawing Edit found unowned posts or attachments')
        test.remove_thread(extra_thread)

        extra_media = number()
        test.add_reply(extra_media, owner, actor, first_comment)
        job3, _, _ = test.queue_job()
        test.approve_job(job3)
        test.attach(extra_media, job3)
        refused('unregistered media receipt', fixture.cleanup_board,
                'Drawing Edit found unowned posts or attachments')
        test.remove_post(extra_media)

        statement(f"UPDATE post_secrets.posting_history SET actor_hash=decode('{foreign}','hex') WHERE post_id={first};")
        refused('foreign posting history actor', fixture.cleanup_board,
                'Drawing Edit found unowned posts or attachments')
        statement(f"UPDATE post_secrets.posting_history SET actor_hash=decode('{actor}','hex') WHERE post_id={first} AND actor_hash=decode('{foreign}','hex');")

        returned = statement(f"INSERT INTO content.reports(board,post_id,reason) VALUES('i',{first},'Synthetic foreign report') RETURNING id;")
        report_numbers = [line for line in returned.splitlines() if line.isascii() and line.isdecimal()]
        must(len(report_numbers) == 1, 'synthetic report not returned exactly once')
        report_id = report_numbers[0]
        refused('spurious report', fixture.cleanup_board,
                'Drawing Edit found unowned posts or attachments')
        statement(f"DELETE FROM content.reports WHERE id={report_id} AND board='i' AND post_id={first} AND reason='Synthetic foreign report';")

        must(board_policy() == fixture.lease['active'], 'fixture changed policy during a refusal')
        fixture.cleanup_board()
        must(not fixture.created, 'successful cleanup did not release the lease')
        test.synthetic_posts.clear()
        test.synthetic_threads.clear()
        assert_empty_i(baseline)
        test.clear_jobs()
        must(outside_rows() == outside, 'qualification changed non-/i/ content')
        print('PASS drawing edit cleanup: exact source row restored, all owned posts/history removed', flush=True)
    finally:
        # This private directory contains only our synthetic 64-byte witnesses.
        # PostgreSQL evidence is deliberately left for inspection on an
        # unexpected failure; never broaden cleanup to suspect foreign rows.
        test.close()


if __name__ == '__main__':
    qualify()
