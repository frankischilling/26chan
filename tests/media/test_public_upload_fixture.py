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
                for table in ('posting_history', 'posting_thread_actions'):
                    self.assertEqual(db.execute(f'SELECT * FROM post_secrets.{table} ORDER BY request_at').fetchall(),
                                     sentinels)
                return mock.Mock()

            with mock.patch('public_upload_fixture.sql', side_effect=sql), \
                    mock.patch.object(fixture, 'upload_one', side_effect=completed_upload) as upload, \
                    mock.patch('public_drawing_fixture.PublicDrawingUpload', side_effect=start_drawing) as drawing:
                fixture.exercise()
            drawing.assert_called_once_with(fixture)
            fixture.drawing_upload.exercise.assert_called_once_with()


if __name__ == '__main__':
    unittest.main()
