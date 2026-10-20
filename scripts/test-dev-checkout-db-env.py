#!/usr/bin/env python3
"""Exercise disposable checkout credential exports with command doubles."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).with_name('dev-checkout-db.sh').read_text()


class CheckoutDevelopmentEnvironmentTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        while True:
            self.cluster = tempfile.TemporaryDirectory(prefix='board-postgres.', dir='/tmp')
            if Path(self.cluster.name).name.removeprefix('board-postgres.').isalnum():
                break
            self.cluster.cleanup()
        self.addCleanup(self.cluster.cleanup)
        self.root = Path(self.directory.name)
        (self.root / 'scripts').mkdir()
        self.script = self.root / 'scripts/dev-checkout-db.sh'
        self.script.write_text(SOURCE)
        self.local = self.root / '.local'
        self.local.mkdir()
        (self.local / 'database.env').write_text('do not execute this file\n')
        (self.local / 'cluster-path').write_text(self.cluster.name + '\n')
        binaries = self.root / 'bin'
        binaries.mkdir()
        mocks = {
            'id': 'printf "0\\n"',
            'openssl': '''[[ $* = 'rand -hex 24' ]] || exit 1
printf 'random\\n' >> "$MOCK_LOG"
printf '%048d\\n' 0 | tr 0 b''',
            'runuser': '''[[ $1 = -u && $2 = postgres && $3 = -- && $4 = /usr/lib/postgresql/16/bin/psql ]] || exit 1
printf 'database\\n' >> "$MOCK_LOG"
if [[ ${*: -1} = 'SHOW data_directory' ]]; then
  printf '%s\\n' "$MOCK_CLUSTER"
else
  cat >/dev/null
fi''',
        }
        for name, body in mocks.items():
            binary = binaries / name
            binary.write_text('#!/usr/bin/env bash\nset -euo pipefail\n' + body + '\n')
            binary.chmod(0o700)
        self.log = self.root / 'calls.log'
        self.env = dict(os.environ, PATH=f'{binaries}:{os.environ["PATH"]}',
                        MOCK_LOG=str(self.log), MOCK_CLUSTER=self.cluster.name)
        self.env.pop('BASH_ENV', None)

    def run_setup(self):
        result = subprocess.run(['bash', str(self.script)], env=self.env,
                                capture_output=True, text=True)
        self.assertNotIn('b' * 48, result.stdout + result.stderr)
        return result

    def test_role_exports_are_private_and_separate_from_other_services(self):
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        for suffix in ['env', 'ps1']:
            adapter = self.local / f'checkout-adapter.{suffix}'
            tests = self.local / f'checkout-tests.{suffix}'
            for path in [adapter, tests]:
                self.assertEqual(path.stat().st_mode & 0o777, 0o600)
                self.assertIn('board_checkout:', path.read_text())
                self.assertNotIn('board_verifier:', path.read_text())
                self.assertNotIn('board_mailer:', path.read_text())
            self.assertIn('TEST_CHECKOUT_DATABASE_URL', tests.read_text())
            self.assertNotIn('TEST_CHECKOUT_DATABASE_URL', adapter.read_text())
        self.assertEqual((self.local / 'database.env').read_text(), 'do not execute this file\n')
        before = self.log.read_text()
        self.assertNotEqual(self.run_setup().returncode, 0)
        self.assertEqual(self.log.read_text(), before, 'restart must not rotate credentials')

    def test_wrong_cluster_refuses_before_generating_credentials(self):
        self.env['MOCK_CLUSTER'] = '/tmp/not-this-cluster'
        self.assertNotEqual(self.run_setup().returncode, 0)
        self.assertEqual(self.log.read_text(), 'database\n')
        self.assertFalse((self.local / 'checkout-tests.env').exists())

    def test_any_existing_role_file_refuses_before_database_calls(self):
        (self.local / 'checkout-adapter.ps1').write_text('owned existing value')
        self.assertNotEqual(self.run_setup().returncode, 0)
        self.assertFalse(self.log.exists())
        self.assertEqual((self.local / 'checkout-adapter.ps1').read_text(), 'owned existing value')


if __name__ == '__main__':
    unittest.main()
