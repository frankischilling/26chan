"""Real SQL predicates and private-file evidence reject foreign drawing jobs."""
import os
import pathlib
import sqlite3
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

from drawing_input_witness import private_input, private_inputs
from public_drawing_fixture import PublicDrawingUpload


class PublicDrawingFixtureTest(unittest.TestCase):
    def fixture(self, mode='ordinary'):
        directory = tempfile.TemporaryDirectory(prefix='26chan-dispatch-')
        self.addCleanup(directory.cleanup)
        root = pathlib.Path(directory.name).resolve()
        quarantine = root / 'quarantine'
        quarantine.mkdir(mode=0o700)
        f = SimpleNamespace(root=root, quarantine=quarantine, intake_user=SimpleNamespace(pw_uid=os.getuid()),
                            ids=[], processes=[], stop=mock.Mock(), intake_unit=object())
        host = SimpleNamespace(f=f, origin='http://127.0.0.1:34567', _poster_id_key='11' * 32)
        value = PublicDrawingUpload(host)
        value.created = True
        value.mode = mode
        db = sqlite3.connect(':memory:')
        self.addCleanup(db.close)
        for schema in ('content', 'post_secrets', 'media', 'media_intake'):
            db.execute(f"ATTACH DATABASE ':memory:' AS {schema}")
        db.create_function('decode', 2, lambda text, encoding: bytes.fromhex(text) if encoding == 'hex' else None)
        db.executescript('''
          CREATE TABLE content.boards(slug TEXT,title TEXT,description TEXT,oekaki BOOLEAN,oekaki_replays BOOLEAN,oekaki_width INT,oekaki_height INT);
          CREATE TABLE content.posts(id INT,board TEXT,thread_id INT,deleted BOOLEAN,comment TEXT);
          CREATE TABLE post_secrets.posting_history(post_id INT,board TEXT,thread_id INT,actor_hash BLOB);
          CREATE TABLE media.jobs(id TEXT,filename TEXT,state TEXT,lease_token TEXT,attempts INT,input_bytes INT);
          CREATE TABLE media_intake.handles(job_id TEXT,capability_hash BLOB);
          CREATE TABLE content.post_media(job_id TEXT);
          CREATE TABLE media.assets(job_id TEXT);
        ''')
        db.execute('INSERT INTO content.boards VALUES (?,?,?,?,?,?,?)',
                   (value.board, 'Drawing qualification', value.marker, True, False, 400, 400))
        db.execute('INSERT INTO content.posts VALUES (?,?,?,?,?)',
                   (41, value.board, 41, False, 'Drawing ownership ' + value.marker))
        db.execute('INSERT INTO post_secrets.posting_history VALUES (?,?,?,?)',
                   (41, value.board, 41, bytes.fromhex(value._posting_actor_hex())))
        return value, db

    def input(self, fixture, db, job='a' * 32, file=True, state='queued', size=40):
        db.execute('INSERT INTO media.jobs VALUES (?,?,?,?,?,?)', (job, 'tegaki.png', state, None, 0, size))
        db.execute('INSERT INTO media_intake.handles VALUES (?,?)', (job, bytes.fromhex('b' * 64)))
        if file:
            path = fixture.f.quarantine / (job + '.input')
            path.write_bytes(bytes(size))
            path.chmod(0o600)
        return job

    def sql(self, db):
        # The production predicate runs unchanged against actual SQL rows;
        # this witness does not claim PostgreSQL lock or dispatch qualification.
        def execute(statement):
            row = db.execute(statement).fetchone()
            return 't' if row == (1,) else 'f' if row == (0,) else '|'.join(map(str, row or []))
        return execute

    def register(self, fixture, db, job='a' * 32, capability_hash='b' * 64):
        target = '0' if fixture.mode == 'ordinary' else '41'
        with mock.patch('public_drawing_fixture.sql', side_effect=self.sql(db)):
            fixture.register_receipt(fixture.marker, job, capability_hash, target, '41')

    def test_separate_actor_receipt_and_private_input_witnesses_are_all_required(self):
        for mode in ('ordinary', 'quick-reply'):
            with self.subTest(mode=mode):
                fixture, db = self.fixture(mode)
                job = self.input(fixture, db)
                self.register(fixture, db)
                self.assertEqual(fixture.f.ids, [job])
                self.assertEqual(fixture.receipts[job]['input'][2], 40)
                self.assertEqual(fixture.receipts[job]['target'], '0' if mode == 'ordinary' else '41')

    def test_real_predicate_rejects_wrong_actor_marker_hash_target_state_size_and_attachment(self):
        changes = [
            ("UPDATE post_secrets.posting_history SET actor_hash=x'00'", 'b' * 64),
            ("UPDATE content.boards SET description='foreign'", 'b' * 64),
            ("UPDATE content.posts SET board='foreign'", 'b' * 64),
            ("UPDATE content.posts SET deleted=1", 'b' * 64),
            ("UPDATE media.jobs SET state='processing',lease_token='foreign',attempts=1", 'b' * 64),
            ("UPDATE media.jobs SET input_bytes=41", 'b' * 64),
            ("UPDATE media.jobs SET filename='foreign.png'", 'b' * 64),
            ("INSERT INTO content.post_media VALUES ('" + 'a' * 32 + "')", 'b' * 64),
            ("SELECT 1", 'c' * 64),
        ]
        for statement, capability_hash in changes:
            with self.subTest(statement=statement):
                fixture, db = self.fixture()
                self.input(fixture, db)
                db.execute(statement)
                with self.assertRaises(AssertionError):
                    self.register(fixture, db, capability_hash=capability_hash)
                self.assertEqual(fixture.f.ids, [])
                self.assertEqual(fixture.receipts, {})
                self.assertEqual(db.execute('SELECT count(*) FROM media.jobs').fetchone(), (1,))

    def test_foreign_database_job_without_owned_quarantine_input_is_untouched(self):
        fixture, db = self.fixture()
        job = self.input(fixture, db, file=False)
        with self.assertRaises(AssertionError):
            self.register(fixture, db)
        self.assertEqual(fixture.f.ids, [])
        self.assertEqual(db.execute('SELECT id FROM media.jobs').fetchall(), [(job,)])

    def test_foreign_marker_wrong_target_and_bad_identifiers_reject_before_sql(self):
        fixture, _ = self.fixture()
        good = [fixture.marker, 'a' * 32, 'b' * 64, '0', '41']
        for index, replacement in [(0, 'c' * 32), (1, 'a' * 31), (2, 'b' * 63), (3, '41'),
                                   (4, '041'), (4, '9223372036854775808'), (4, '41;DELETE')]:
            values = list(good)
            values[index] = replacement
            with self.subTest(index=index), mock.patch('public_drawing_fixture.sql') as sql:
                with self.assertRaises(AssertionError):
                    fixture.register_receipt(*values)
                sql.assert_not_called()

    def test_input_rejects_symlink_hardlink_wrong_owner_permissions_and_oversize(self):
        for kind in ('symlink', 'hardlink', 'fifo', 'owner', 'permissions', 'oversize'):
            with self.subTest(kind=kind):
                fixture, db = self.fixture()
                job = self.input(fixture, db)
                path = fixture.f.quarantine / (job + '.input')
                if kind == 'symlink':
                    external = fixture.f.root / 'external'
                    path.rename(external)
                    path.symlink_to(external)
                elif kind == 'hardlink':
                    os.link(path, fixture.f.root / 'external')
                elif kind == 'fifo':
                    path.unlink()
                    os.mkfifo(path, 0o600)
                elif kind == 'owner':
                    fixture.f.intake_user.pw_uid += 1
                elif kind == 'permissions':
                    path.chmod(0o644)
                else:
                    with path.open('r+b') as stream:
                        stream.truncate(8388609)
                with self.assertRaises((AssertionError, OSError)):
                    self.register(fixture, db)
                self.assertEqual(fixture.f.ids, [])
                self.assertEqual(db.execute('SELECT count(*) FROM media.jobs').fetchone(), (1,))

    def test_quarantine_cannot_escape_run_root_or_be_symlinked(self):
        fixture, db = self.fixture()
        job = self.input(fixture, db)
        for quarantine in (fixture.f.root, fixture.f.root / '..' / 'quarantine'):
            with self.assertRaises(AssertionError):
                private_input(fixture.f.root, quarantine, os.getuid(), job)
        real = fixture.f.root / 'other'
        fixture.f.quarantine.rename(real)
        fixture.f.quarantine.symlink_to(real, target_is_directory=True)
        with self.assertRaises(AssertionError):
            private_input(fixture.f.root, fixture.f.quarantine, os.getuid(), job)

    def test_completed_pre_receipt_upload_is_recovered_without_claiming_actor_linkage(self):
        fixture, db = self.fixture()
        baseline = self.input(fixture, db, 'c' * 32)
        fixture.recovery_baseline = private_inputs(fixture.f.root, fixture.f.quarantine, os.getuid())
        orphan = self.input(fixture, db)
        foreign = self.input(fixture, db, 'd' * 32, file=False)
        with mock.patch('public_drawing_fixture.sql', side_effect=self.sql(db)):
            fixture.recover_unregistered_inputs()
        self.assertEqual(fixture.f.ids, [orphan])
        self.assertIsNone(fixture.recovery_baseline)
        self.assertEqual(db.execute('SELECT id FROM media.jobs ORDER BY id').fetchall(), [(orphan,), (baseline,), (foreign,)])
        self.assertEqual(fixture.receipts, {})

    def test_complete_input_before_finish_upload_commit_recovers_receiving_row(self):
        fixture, db = self.fixture()
        fixture.recovery_baseline = set()
        job = self.input(fixture, db, state='receiving')
        db.execute('UPDATE media.jobs SET input_bytes=NULL')
        with mock.patch('public_drawing_fixture.sql', side_effect=self.sql(db)):
            fixture.recover_unregistered_inputs()
        self.assertEqual(fixture.f.ids, [job])
        self.assertEqual(db.execute('SELECT state,input_bytes FROM media.jobs').fetchone(), ('receiving', None))

    def test_pre_receipt_recovery_rejects_leased_approved_attached_and_mismatched_private_inputs(self):
        for statement in ("UPDATE media.jobs SET state='processing',lease_token='lease',attempts=1",
                          "UPDATE media.jobs SET input_bytes=41",
                          "INSERT INTO media.assets VALUES ('" + 'a' * 32 + "')",
                          "INSERT INTO content.post_media VALUES ('" + 'a' * 32 + "')"):
            with self.subTest(statement=statement):
                fixture, db = self.fixture()
                fixture.recovery_baseline = set()
                self.input(fixture, db)
                db.execute(statement)
                with mock.patch('public_drawing_fixture.sql', side_effect=self.sql(db)), self.assertRaises(AssertionError):
                    fixture.recover_unregistered_inputs()
                self.assertEqual(fixture.f.ids, [])
                self.assertEqual(db.execute('SELECT count(*) FROM media.jobs').fetchone(), (1,))

    def test_interrupted_browser_cleanup_stops_browser_and_intake_before_orphan_snapshot(self):
        fixture, db = self.fixture()
        fixture.recovery_baseline = set()
        job = self.input(fixture, db)
        fixture.browser = object()
        calls = []
        fixture.f.stop = lambda process: calls.append('intake' if process is fixture.f.intake_unit else 'browser')
        execute = self.sql(db)
        def queried(statement):
            self.assertEqual(calls, ['browser', 'intake'])
            return execute(statement)
        with mock.patch('public_drawing_fixture.sql', side_effect=queried), mock.patch('public_upload_fixture.PublicUpload.cleanup'):
            fixture.cleanup()
        self.assertEqual(fixture.f.ids, [job])

    def test_suspect_recovery_still_stops_every_owned_process_and_retains_evidence(self):
        fixture, db = self.fixture()
        fixture.recovery_baseline = set()
        job = self.input(fixture, db, state='processing')
        db.execute("UPDATE media.jobs SET lease_token='leased',attempts=1")
        browser, public, other = object(), object(), object()
        fixture.browser = browser
        fixture.f.processes = [public, fixture.f.intake_unit, other, browser]
        calls = []
        fixture.f.stop = calls.append
        with mock.patch('public_drawing_fixture.sql', side_effect=self.sql(db)), self.assertRaisesRegex(RuntimeError, 'evidence retained'):
            fixture.cleanup()
        self.assertTrue(all(process in calls for process in fixture.f.processes))
        self.assertEqual(fixture.f.ids, [])
        self.assertEqual(db.execute('SELECT id FROM media.jobs').fetchall(), [(job,)])
        self.assertTrue((fixture.f.quarantine / (job + '.input')).is_file())
        self.assertIsNotNone(fixture.recovery_baseline)

    def test_replaced_private_input_refuses_revoked_cleanup_before_database_mutation(self):
        fixture, db = self.fixture()
        job = self.input(fixture, db)
        self.register(fixture, db)
        path = fixture.f.quarantine / (job + '.input')
        replacement = fixture.f.quarantine / 'replacement'
        replacement.write_bytes(bytes(40))
        replacement.chmod(0o600)
        replacement.replace(path)
        with mock.patch('public_drawing_fixture.sql') as sql, self.assertRaises(AssertionError):
            fixture.remove_revoked_queued(fixture.marker, job)
        sql.assert_not_called()

    def test_unregistered_foreign_job_cannot_reach_cleanup_or_dispatch(self):
        fixture, db = self.fixture()
        self.input(fixture, db, file=False)
        with mock.patch('public_drawing_fixture.sql') as sql:
            for action in (fixture.remove_revoked_queued, fixture.approve_receipt):
                with self.assertRaises(AssertionError):
                    action(fixture.marker, 'a' * 32)
            sql.assert_not_called()


if __name__ == '__main__':
    unittest.main()
