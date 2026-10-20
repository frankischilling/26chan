"""Lease and private-service wiring checks; no VM or PostgreSQL runs here."""
import json
import pathlib
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock

from public_vip_jpeg_fixture import PublicVipJpeg


class PublicVipJpegTest(unittest.TestCase):
    def fixture(self):
        return PublicVipJpeg(SimpleNamespace(f=SimpleNamespace()))

    def test_lease_changes_only_owned_marker_and_captcha_on_the_source_board(self):
        fixture = self.fixture()
        before = {'slug': 'p', 'staff_only': False, 'captcha_enabled': True, 'description': 'source'}
        with mock.patch('public_vip_jpeg_fixture.sql', side_effect=[json.dumps(before), '']) as sql:
            fixture.lease_board()
        self.assertEqual(fixture.policy['before'], before)
        self.assertEqual(fixture.policy['active'], {**before, 'description': fixture.filename, 'captcha_enabled': False})
        statement = sql.call_args_list[1].args[0]
        for clause in ['standard_conforming_strings=on', 'FOR UPDATE NOWAIT', 'to_jsonb(b)', 'IS DISTINCT FROM',
                       'content.posts', 'content.threads', 'post_secrets.posting_history',
                       'post_secrets.posting_thread_actions']:
            self.assertIn(clause, statement)
        self.assertIn("WHERE slug='p'", statement)

    def test_invalid_filename_is_rejected_before_sql(self):
        fixture = self.fixture()
        for filename in ['outside.jpg', "public-vip-jpeg-'bad.jpg", 'public-vip-jpeg-' + 'a'*32 + '.jpg\n']:
            fixture.filename = filename
            with mock.patch('public_vip_jpeg_fixture.sql') as sql:
                with self.assertRaises(AssertionError):
                    fixture.lease_board()
                sql.assert_not_called()

    def test_source_board_assumptions_are_checked_before_mutation(self):
        for row in [{'slug': 'other', 'staff_only': False, 'captcha_enabled': True},
                    {'slug': 'p', 'staff_only': True, 'captcha_enabled': True},
                    {'slug': 'p', 'staff_only': False, 'captcha_enabled': False}]:
            fixture = self.fixture()
            with mock.patch('public_vip_jpeg_fixture.sql', return_value=json.dumps(row)) as sql:
                with self.assertRaises(AssertionError):
                    fixture.lease_board()
                self.assertEqual(sql.call_count, 1)
                self.assertIsNone(fixture.policy)

    def test_recovery_state_precedes_ambiguous_commit(self):
        fixture = self.fixture()
        before = {'slug': 'p', 'staff_only': False, 'captcha_enabled': True, 'description': 'source'}
        with mock.patch('public_vip_jpeg_fixture.sql', side_effect=[json.dumps(before), RuntimeError('owned uncertainty')]):
            with self.assertRaises(RuntimeError):
                fixture.lease_board()
        self.assertEqual(fixture.policy['before'], before)
        with mock.patch('public_vip_jpeg_fixture.sql', side_effect=RuntimeError('owned restore uncertainty')):
            with self.assertRaises(RuntimeError):
                fixture.restore_board()
        self.assertIsNotNone(fixture.policy)
        with mock.patch('public_vip_jpeg_fixture.sql') as sql:
            fixture.restore_board()
            self.assertIn('IS DISTINCT FROM', sql.call_args.args[0])
            self.assertIn('cannot restore changed or occupied policy', sql.call_args.args[0])
        self.assertIsNone(fixture.policy)

    def test_changed_policy_stops_cleanup_before_any_deletion(self):
        fixture = self.fixture()
        fixture.policy = {'before': {'description': 'source'}, 'active': {'description': fixture.filename}}
        with mock.patch('public_vip_jpeg_fixture.sql', return_value=json.dumps({'description': 'foreign'})) as sql:
            with self.assertRaises(AssertionError):
                fixture.cleanup()
            self.assertEqual(sql.call_count, 1)
            self.assertTrue(sql.call_args.args[0].startswith('SELECT'))

    def test_private_issuer_and_operator_receive_only_their_own_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / 'bin').mkdir(); (root / 'binaries').mkdir()
            (root / 'binaries/board-verification').write_bytes(b'owned unused executable fixture')
            before = b'APP_ENV=development\nDATABASE_URL=owned-public\n'
            (root / 'public.env').write_bytes(before)
            writer = lambda path, data: path.write_bytes(data if isinstance(data, bytes) else data.encode())
            f = SimpleNamespace(root=root, bin=root/'bin', binaries=root/'binaries',
                intake_user=SimpleNamespace(pw_uid=11), coordinator=SimpleNamespace(pw_uid=12),
                gateway=SimpleNamespace(pw_uid=13), http_user=SimpleNamespace(pw_uid=14),
                launch=mock.Mock(return_value=mock.Mock()), write=mock.Mock(side_effect=writer))
            host = SimpleNamespace(f=f, _public_identity=(15, 15), unit=SimpleNamespace(name='owned-public'), ready=lambda: True)
            fixture = PublicVipJpeg(host)
            environment = {'VERIFICATION_DATABASE_URL':'owned-issuer', 'MIGRATION_DATABASE_URL':'owned-operator'}
            with mock.patch.dict('os.environ', environment), \
                    mock.patch('public_vip_jpeg_fixture.account', return_value=SimpleNamespace(pw_uid=16)), \
                    mock.patch('public_vip_jpeg_fixture.wait_until'), \
                    mock.patch('public_vip_jpeg_fixture.systemctl'), \
                    mock.patch('public_vip_jpeg_fixture.subprocess.run', return_value=SimpleNamespace(returncode=0, stdout=b'{"id":9}')) as run:
                fixture.setup_issuer()
            issuer_environment = f.launch.call_args.args[2]
            self.assertEqual(set(issuer_environment), {'PATH','APP_ENV','VERIFICATION_BIND','VERIFICATION_DATABASE_URL','VERIFICATION_SERVICE_TOKEN','VERIFICATION_KEY','VERIFICATION_MAIL_KEY'})
            self.assertEqual(issuer_environment['VERIFICATION_DATABASE_URL'], 'owned-issuer')
            self.assertEqual(run.call_args.args[0], [root/'binaries/board-vip-admin'])
            operation = json.loads(run.call_args.kwargs['input'])
            self.assertEqual(operation, {'action':'create','identifier':fixture.identifier,'name':'Owned Verified JPEG','password':fixture.password})
            self.assertEqual(set(run.call_args.kwargs['env']), {'PATH','APP_ENV','MIGRATION_DATABASE_URL','VERIFICATION_KEY'})
            active = (root/'public.env').read_bytes()
            self.assertTrue(active.startswith(before))
            self.assertIn(fixture.token.encode(), active)
            for value in [fixture.key, fixture.mail_key, fixture.password, 'owned-issuer', 'owned-operator']:
                self.assertNotIn(value.encode(), active)
            self.assertEqual(fixture.account_id, 9)

    def test_cleanup_never_overwrites_an_unexpected_public_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory); (root/'public.env').write_bytes(b'foreign')
            fixture = self.fixture(); fixture.f.root = root
            fixture.public_env = {'before': b'original', 'active': b'owned'}
            with mock.patch('public_vip_jpeg_fixture.sql', return_value=''), \
                    mock.patch('public_vip_jpeg_fixture.systemctl') as systemctl:
                with self.assertRaises(AssertionError):
                    fixture.cleanup()
                systemctl.assert_not_called()
            self.assertEqual((root/'public.env').read_bytes(), b'foreign')

    def test_cli_and_browser_do_not_receive_private_credentials_in_arguments(self):
        # Review the executable fixture boundary, not a substitute for its actual
        # isolated-host run. Shell commands and broad environment copies stay out.
        source = pathlib.Path(__file__).with_name('public_vip_jpeg_fixture.py').read_text()
        self.assertIn("operation = {'action': 'create'", source)
        self.assertIn('input=json.dumps(operation).encode()', source)
        self.assertIn("self.f.launch([node, REPO / 'tests/browser/source-vip-jpeg.mjs', config]", source)
        self.assertNotIn('shell=True', source)
        self.assertNotIn('os.environ.copy()', source)


if __name__ == '__main__':
    unittest.main()
