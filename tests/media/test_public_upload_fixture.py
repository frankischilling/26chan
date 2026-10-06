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
                deletion_max_seconds INTEGER NOT NULL DEFAULT 1800
            )''')
            db.execute("INSERT INTO content.boards(slug) VALUES ('a')")
            fixture = self.fixture()
            with mock.patch('public_upload_fixture.sql', side_effect=db.execute) as sql:
                fixture.create_board()
            sql.assert_called_once()
            self.assertTrue(fixture.created)
            self.assertEqual(db.execute('''SELECT slug, deletion_known_min_seconds,
                deletion_unknown_min_seconds, deletion_max_seconds
                FROM content.boards ORDER BY slug''').fetchall(),
                [('a', 60, 600, 1800), (fixture.board, 0, 0, 1800)])

    def test_non_synthetic_slugs_are_rejected_before_sql(self):
        for board in ('a', 'demo', 'fixture', 'u12345678-extra', 'u12345678\n'):
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


if __name__ == '__main__':
    unittest.main()
