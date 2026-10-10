"""The disposable media board explicitly permits immediate cleanup coverage."""
import pathlib
import sqlite3
import unittest
from types import SimpleNamespace
from unittest import mock

from public_upload_fixture import PublicUpload


class PublicUploadBoardTest(unittest.TestCase):
    def fixture(self):
        return PublicUpload(SimpleNamespace(root=pathlib.Path('/tmp/26chan-dispatch-12345678')))

    def test_only_owned_board_overrides_minimum_ages(self):
        # A local SQL witness for fixture scoping, not a substitute for the
        # PostgreSQL source-policy and authenticated media integration tests.
        with sqlite3.connect(':memory:') as db:
            db.execute("ATTACH DATABASE ':memory:' AS content")
            db.execute('''CREATE TABLE content.boards (
                slug TEXT PRIMARY KEY, title TEXT, description TEXT,
                max_comment_chars INTEGER, reply_limit INTEGER, bump_limit INTEGER,
                thread_limit INTEGER, threads_per_page INTEGER, image_limit INTEGER,
                comment_spoiler_cleanup BOOLEAN,
                deletion_known_min_seconds INTEGER NOT NULL DEFAULT 60,
                deletion_unknown_min_seconds INTEGER NOT NULL DEFAULT 600,
                deletion_max_seconds INTEGER NOT NULL DEFAULT 1800,
                posting_reply_seconds INTEGER NOT NULL DEFAULT 60,
                posting_image_seconds INTEGER NOT NULL DEFAULT 60,
                posting_thread_seconds INTEGER NOT NULL DEFAULT 600
            )''')
            db.execute("INSERT INTO content.boards(slug) VALUES ('a')")
            fixture = self.fixture()
            with mock.patch('public_upload_fixture.sql', side_effect=db.execute) as sql:
                fixture.create_board()
            sql.assert_called_once()
            self.assertTrue(fixture.created)
            self.assertEqual(db.execute('''SELECT slug, deletion_known_min_seconds,
                deletion_unknown_min_seconds, deletion_max_seconds,
                posting_reply_seconds, posting_image_seconds, posting_thread_seconds
                FROM content.boards ORDER BY slug''').fetchall(),
                [('a', 60, 600, 1800, 60, 60, 600), (fixture.board, 0, 0, 1800, 0, 0, 0)])

    def test_non_synthetic_slugs_are_rejected_before_sql(self):
        for board in ('a', 'demo', 'fixture', 'u12345678-extra', 'u12345678\n', 'uffffffff'):
            with self.subTest(board=board):
                fixture = self.fixture()
                fixture.board = board
                with mock.patch('public_upload_fixture.sql') as sql:
                    with self.assertRaises(AssertionError):
                        fixture.create_board()
                sql.assert_not_called()
                self.assertFalse(fixture.created)

    def test_failed_insert_does_not_mark_board_created(self):
        fixture = self.fixture()
        with mock.patch('public_upload_fixture.sql', side_effect=AssertionError('insert failed')):
            with self.assertRaisesRegex(AssertionError, '^insert failed$'):
                fixture.create_board()
        self.assertFalse(fixture.created)

    def test_fresh_key_ignores_external_key_and_matches_domain_vector(self):
        with mock.patch.dict('os.environ', {'POSTER_ID_KEY': 'aa' * 32}):
            with mock.patch('public_upload_fixture.secrets.token_hex',
                            side_effect=['12345678', '11' * 32]) as random:
                fixture = self.fixture()
            self.assertEqual(random.call_args_list, [mock.call(4), mock.call(32)])
        self.assertEqual(fixture._actor_hex(),
                         'b371a09e65a56258509ae54ee34f375d5f4f34132287415fac784007dfe86467')
        self.assertNotEqual(self.fixture()._actor_hex(), self.fixture()._actor_hex())

    def test_reset_removes_only_owned_actor_and_preserves_foreign_sentinel(self):
        fixture = self.fixture()
        fixture.created = True
        foreign = self.fixture()._actor_hex()
        with sqlite3.connect(':memory:') as db:
            db.execute("ATTACH DATABASE ':memory:' AS content")
            db.execute("ATTACH DATABASE ':memory:' AS post_secrets")
            db.create_function('decode', 2, lambda value, encoding: bytes.fromhex(value)
                               if encoding == 'hex' else None)
            db.execute('''CREATE TABLE content.boards (slug TEXT PRIMARY KEY,
                deletion_known_min_seconds INTEGER, deletion_unknown_min_seconds INTEGER,
                deletion_max_seconds INTEGER)''')
            db.execute('INSERT INTO content.boards VALUES (?,0,0,1800)', (fixture.board,))
            db.execute('''CREATE TABLE post_secrets.public_deletion_actors (
                actor_hash BLOB PRIMARY KEY, events TEXT, expires_at INTEGER)''')
            db.executemany('INSERT INTO post_secrets.public_deletion_actors VALUES (?,?,?)',
                           [(bytes.fromhex(fixture._actor_hex()), '1,2,3', 86403),
                            (bytes.fromhex(foreign), '41,42', 86442)])
            before = db.execute('SELECT * FROM post_secrets.public_deletion_actors WHERE actor_hash=?',
                                (bytes.fromhex(foreign),)).fetchone()

            def sql(statement):
                cursor = db.execute(statement)
                return '|'.join(map(str, cursor.fetchone())) if cursor.description else ''

            with mock.patch('public_upload_fixture.sql', side_effect=sql):
                fixture.reset_deletion_quota()
                # Cleanup after the final workflow uses the same scoped operation.
                fixture.reset_deletion_quota()
            self.assertEqual(db.execute('SELECT * FROM post_secrets.public_deletion_actors').fetchall(),
                             [before])
            self.assertEqual(db.execute('SELECT * FROM content.boards').fetchall(),
                             [(fixture.board, 0, 0, 1800)])

    def test_reset_rejects_unowned_board_and_uncreated_fixture(self):
        fixture = self.fixture()
        with mock.patch('public_upload_fixture.sql') as sql:
            with self.assertRaises(AssertionError):
                fixture.reset_deletion_quota()
            fixture.created = True
            fixture.board = 'uffffffff'
            with self.assertRaises(AssertionError):
                fixture.reset_deletion_quota()
        sql.assert_not_called()

    def test_reset_rejects_changed_age_policy_before_deletion(self):
        fixture = self.fixture()
        fixture.created = True
        for policy in ('60|600|1800', '0|0|1801', ''):
            with self.subTest(policy=policy), mock.patch('public_upload_fixture.sql', return_value=policy) as sql:
                with self.assertRaises(AssertionError):
                    fixture.reset_deletion_quota()
                self.assertEqual(sql.call_count, 1)
                self.assertTrue(sql.call_args.args[0].startswith('SELECT '))

    def test_posting_actor_is_separate_and_matches_loopback_domain_vector(self):
        fixture = self.fixture()
        fixture._poster_id_key = '11' * 32
        self.assertEqual(fixture._posting_actor_hex(),
                         '86dc8cf300333e51f602b95c5c4833cefb6d92d1e19b2431f5025558004e4d51')
        self.assertNotEqual(fixture._posting_actor_hex(), fixture._actor_hex())
        self.assertNotEqual(self.fixture()._posting_actor_hex(), self.fixture()._posting_actor_hex())

    def test_posting_reset_preserves_other_actors_and_other_boards(self):
        fixture = self.fixture()
        fixture.created = True
        own = bytes.fromhex(fixture._posting_actor_hex())
        foreign = bytes.fromhex(self.fixture()._posting_actor_hex())
        with sqlite3.connect(':memory:') as db:
            db.execute("ATTACH DATABASE ':memory:' AS content")
            db.execute("ATTACH DATABASE ':memory:' AS post_secrets")
            db.create_function('decode', 2, lambda value, encoding: bytes.fromhex(value)
                               if encoding == 'hex' else None)
            db.execute('CREATE TABLE content.boards (slug TEXT PRIMARY KEY, posting_reply_seconds INTEGER, posting_image_seconds INTEGER, posting_thread_seconds INTEGER)')
            db.execute('INSERT INTO content.boards VALUES (?,0,0,0)', (fixture.board,))
            sentinels = [(foreign, fixture.board, 42), (own, 'foreign', 43)]
            for table in ('posting_history', 'posting_thread_actions'):
                db.execute(f'CREATE TABLE post_secrets.{table} (actor_hash BLOB, board TEXT, request_at INTEGER)')
                db.executemany(f'INSERT INTO post_secrets.{table} VALUES (?,?,?)',
                               [(own, fixture.board, 41), *sentinels])

            def sql(statement):
                cursor = db.execute(statement)
                return '|'.join(map(str, cursor.fetchone())) if cursor.description else ''

            with mock.patch('public_upload_fixture.sql', side_effect=sql):
                fixture.reset_posting_history()
                fixture.reset_posting_history()
            for table in ('posting_history', 'posting_thread_actions'):
                self.assertEqual(db.execute(f'SELECT * FROM post_secrets.{table} ORDER BY request_at').fetchall(), sentinels)

    def test_posting_reset_rejects_missing_ownership_and_changed_policy(self):
        fixture = self.fixture()
        with mock.patch('public_upload_fixture.sql') as sql:
            with self.assertRaises(AssertionError):
                fixture.reset_posting_history()
            fixture.created = True
            fixture.board = 'uffffffff'
            with self.assertRaises(AssertionError):
                fixture.reset_posting_history()
        sql.assert_not_called()
        fixture.board = fixture._owned_board
        for policy in ('60|60|600', '0|0|1', ''):
            with self.subTest(policy=policy), mock.patch('public_upload_fixture.sql', return_value=policy) as sql:
                with self.assertRaises(AssertionError):
                    fixture.reset_posting_history()
                self.assertEqual(sql.call_count, 1)
                self.assertTrue(sql.call_args.args[0].startswith('SELECT '))

    def test_drawing_transition_clears_completed_host_actor_board_only(self):
        # Inert SQL scope/order witness. Actual PostgreSQL admission and HTTP
        # responses remain separate integration qualifications.
        fixture = self.fixture()
        fixture.created = True
        own = bytes.fromhex(fixture._posting_actor_hex())
        foreign = bytes.fromhex(self.fixture()._posting_actor_hex())
        with sqlite3.connect(':memory:') as db:
            db.execute("ATTACH DATABASE ':memory:' AS content")
            db.execute("ATTACH DATABASE ':memory:' AS post_secrets")
            db.create_function('decode', 2, lambda value, encoding: bytes.fromhex(value)
                               if encoding == 'hex' else None)
            db.execute('''CREATE TABLE content.boards (slug TEXT PRIMARY KEY, title TEXT,
                comment_spoiler_cleanup BOOLEAN, posting_reply_seconds INTEGER,
                posting_image_seconds INTEGER, posting_thread_seconds INTEGER)''')
            db.execute("INSERT INTO content.boards VALUES (?,'Upload qualification',true,0,0,0)",
                       (fixture.board,))
            sentinels = [(foreign, fixture.board, 42), (own, 'foreign', 43)]
            for table in ('posting_history', 'posting_thread_actions'):
                db.execute(f'CREATE TABLE post_secrets.{table} (actor_hash BLOB, board TEXT, request_at INTEGER)')
                db.executemany(f'INSERT INTO post_secrets.{table} VALUES (?,?,?)', sentinels)

            def sql(statement):
                cursor = db.execute(statement)
                return '|'.join(map(str, cursor.fetchone())) if cursor.description else ''

            def completed_upload(*args, **kwargs):
                for table in ('posting_history', 'posting_thread_actions'):
                    db.execute(f'INSERT INTO post_secrets.{table} VALUES (?,?,?)', (own, fixture.board, 41))

            def start_drawing(host):
                self.assertIs(host, fixture)
                self.assertEqual(upload.call_count, 9)
                restart.assert_called_once_with()
                for table in ('posting_history', 'posting_thread_actions'):
                    self.assertEqual(db.execute(f'SELECT * FROM post_secrets.{table} ORDER BY request_at').fetchall(),
                                     sentinels)
                return mock.Mock()

            with mock.patch('public_upload_fixture.sql', side_effect=sql), \
                    mock.patch.object(fixture, 'upload_one', side_effect=completed_upload) as upload, \
                    mock.patch.object(fixture, 'restart_for_drawing') as restart, \
                    mock.patch('public_drawing_fixture.PublicDrawingUpload', side_effect=start_drawing) as drawing:
                fixture.exercise()
            drawing.assert_called_once_with(fixture)
            fixture.drawing_upload.exercise.assert_called_once_with()


class DrawingQuotaBoundaryTest(unittest.TestCase):
    def fixture(self):
        fixture = PublicUpload(SimpleNamespace(root=pathlib.Path('/tmp/26chan-dispatch-12345678')))
        fixture.installed = fixture.created = True
        fixture._public_identity = (1001, 1002)
        fixture.f.processes = [fixture.unit]
        fixture.f.unit_files = {pathlib.Path('/run/systemd/system') / fixture.unit.name: b'owned public unit'}
        fixture.completed_jobs = [format(i, '032x') for i in range(1, 10)]
        fixture.f.ids = list(fixture.completed_jobs)
        fixture.filenames = [f'public-upload-{fixture.board}.{suffix}' for suffix in
                            ('png', 'baseline.jpg', 'progressive.jpg', 'animated.gif', 'tracking.png',
                             'quick-reply.png', 'quick-reply-inline.png', 'quick-reply-disabled.png',
                             'quick-reply-inline-disabled.png')]
        fixture.browser = SimpleNamespace(poll=lambda: 0)
        fixture.f.clean_vm = mock.Mock()
        return fixture

    def test_restart_is_owned_completed_and_gets_a_new_invocation(self):
        fixture = self.fixture()
        states = [dict(ActiveState='active', InvocationID='1'*32), dict(ActiveState='active', InvocationID='2'*32, MainPID='12345')]
        with mock.patch.object(pathlib.Path, 'read_bytes', return_value=b'owned public unit'), \
                mock.patch.object(pathlib.Path, 'read_text', return_value='Uid: 1001 1001 1001 1001\nGid: 1002 1002 1002 1002'), \
                mock.patch('public_upload_fixture.sql', return_value='t') as sql, \
                mock.patch('public_upload_fixture.systemctl') as systemctl, \
                mock.patch('public_upload_fixture.wait_until') as ready, \
                mock.patch.object(fixture.unit, 'state', side_effect=states):
            fixture.restart_for_drawing()
        self.assertIn("count(*)=9 AND bool_and(state='published'", sql.call_args.args[0])
        self.assertNotIn('DELETE', sql.call_args.args[0])
        fixture.f.clean_vm.assert_called_once_with()
        systemctl.assert_called_once_with('restart', fixture.unit.name, timeout=35)
        ready.assert_called_once_with(fixture.ready)
        self.assertIsNone(fixture.browser)

    def test_incomplete_browser_jobs_or_vm_and_changed_unit_prevent_restart(self):
        for failure in ('browser', 'missing-job', 'live-job', 'vm', 'unit'):
            fixture = self.fixture()
            if failure == 'browser': fixture.browser = SimpleNamespace(poll=lambda: None)
            if failure == 'missing-job': fixture.completed_jobs.pop()
            if failure == 'vm': fixture.f.clean_vm.side_effect = AssertionError('live VM')
            with self.subTest(failure=failure), \
                    mock.patch.object(pathlib.Path, 'read_bytes', return_value=b'changed' if failure == 'unit' else b'owned public unit'), \
                    mock.patch('public_upload_fixture.sql', return_value='f' if failure == 'live-job' else 't'), \
                    mock.patch('public_upload_fixture.systemctl') as systemctl:
                with self.assertRaises(AssertionError): fixture.restart_for_drawing()
                systemctl.assert_not_called()

    def test_fresh_invocation_with_wrong_runtime_identity_is_rejected(self):
        fixture = self.fixture()
        states = [dict(ActiveState='active', InvocationID='1'*32), dict(ActiveState='active', InvocationID='2'*32, MainPID='12345')]
        with mock.patch.object(pathlib.Path, 'read_bytes', return_value=b'owned public unit'), \
                mock.patch.object(pathlib.Path, 'read_text', return_value='Uid: 0 0 0 0\nGid: 1002 1002 1002 1002'), \
                mock.patch('public_upload_fixture.sql', return_value='t'), \
                mock.patch('public_upload_fixture.systemctl'), \
                mock.patch('public_upload_fixture.wait_until'), \
                mock.patch.object(fixture.unit, 'state', side_effect=states):
            with self.assertRaises(AssertionError): fixture.restart_for_drawing()
        self.assertIsNotNone(fixture.browser)

    def test_unchanged_invocation_is_not_a_successful_boundary(self):
        fixture = self.fixture()
        with mock.patch.object(pathlib.Path, 'read_bytes', return_value=b'owned public unit'), \
                mock.patch.object(pathlib.Path, 'read_text', return_value='Uid: 1001 1001 1001 1001\nGid: 1002 1002 1002 1002'), \
                mock.patch('public_upload_fixture.sql', return_value='t'), \
                mock.patch('public_upload_fixture.systemctl'), \
                mock.patch('public_upload_fixture.wait_until'), \
                mock.patch.object(fixture.unit, 'state', return_value=dict(ActiveState='active', InvocationID='1'*32)):
            with self.assertRaisesRegex(AssertionError, 'fresh invocation'): fixture.restart_for_drawing()
        self.assertIsNotNone(fixture.browser)


class DrawingEditDiagnosticTest(unittest.TestCase):
    def test_only_complete_fixed_drawing_diagnostics_are_reported(self):
        from public_upload_fixture import finish_browser
        good = 'cancel=403 ui=cancel-error editor=hidden cursor=hidden active=false'
        for script, line, expected in [
            ('drawing-upload.mjs', good, '(Edit ' + good + ')'),
            ('drawing-upload.mjs', 'unavailable', '(Edit unavailable)'),
            ('drawing-upload.mjs', good + ' secret-capability', None),
            ('drawing-upload.mjs', good.replace('cancel-error', 'secret-capability'), None),
            ('public-upload.mjs', good, None),
        ]:
            process = SimpleNamespace(returncode=1, communicate=lambda **_: (b'', ('OWNED_DRAWING_EDIT ' + line + '\n').encode()))
            with self.subTest(script=script, line=line), self.assertRaises(AssertionError) as failure:
                finish_browser(process, script)
            message = str(failure.exception)
            self.assertNotIn('secret-capability', message)
            if expected:
                self.assertIn(expected, message)
            else:
                self.assertNotIn('(Edit ', message)


if __name__ == '__main__':
    unittest.main()
