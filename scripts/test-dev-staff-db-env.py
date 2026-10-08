#!/usr/bin/env python3
"""Check staff bootstrap identity files without PostgreSQL or real credentials."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).with_name('dev-staff-db.sh').read_text()
KEY = '0123456789abcdef' * 4
PASSWORD = 'b' * 48


class StaffDevelopmentEnvironmentTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        # tempfile may use underscores, while the real mktemp cluster suffix is
        # alphanumeric and the bootstrap intentionally enforces that shape.
        while True:
            self.cluster = tempfile.TemporaryDirectory(prefix='board-postgres.', dir='/tmp')
            if Path(self.cluster.name).name.removeprefix('board-postgres.').isalnum():
                break
            self.cluster.cleanup()
        self.addCleanup(self.cluster.cleanup)
        self.root = Path(self.directory.name)
        (self.root / 'scripts').mkdir()
        self.script = self.root / 'scripts/dev-staff-db.sh'
        self.script.write_text(SOURCE)
        self.local = self.root / '.local'
        self.local.mkdir()
        self.database = self.local / 'database.env'
        self.database.write_text(f'export POSTER_ID_KEY={KEY}\n')
        self.database.chmod(0o600)
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
            executable = binaries / name
            executable.write_text('#!/usr/bin/env bash\nset -euo pipefail\n' + body + '\n')
            executable.chmod(0o700)
        self.log = self.root / 'calls.log'
        self.env = dict(os.environ, PATH=f'{binaries}:{os.environ["PATH"]}',
                        MOCK_LOG=str(self.log), MOCK_CLUSTER=self.cluster.name)
        self.env.pop('BASH_ENV', None)
        self.env.pop('STAFF_POSTER_ID_KEY', None)

    def run_setup(self):
        result = subprocess.run(['bash', str(self.script)], env=self.env, input='',
                                capture_output=True, text=True)
        for value in (KEY, PASSWORD):
            self.assertNotIn(value, result.stdout + result.stderr)
        return result

    def test_matching_private_identity_exports_and_restart(self):
        # Arbitrary file contents must not execute, even though setup runs as root.
        marker = self.root / 'executed'
        self.database.write_text(f'touch "{marker}"\nexport POSTER_ID_KEY={KEY}\n')
        result = self.run_setup()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(marker.exists())
        shell = self.local / 'staff.env'
        powershell = self.local / 'staff.ps1'
        self.assertEqual(shell.read_text().count('export STAFF_POSTER_ID_KEY='), 1)
        self.assertIn(f'export STAFF_POSTER_ID_KEY={KEY}\n', shell.read_text())
        self.assertIn(f"$env:STAFF_POSTER_ID_KEY = '{KEY}'\n", powershell.read_text())
        self.assertIn('export AUTH_DATABASE_URL=', shell.read_text())
        self.assertIn('export STAFF_DATABASE_URL=', shell.read_text())
        self.assertIn('$env:AUTH_DATABASE_URL = ', powershell.read_text())
        self.assertIn('$env:STAFF_DATABASE_URL = ', powershell.read_text())
        for file in (shell, powershell, self.database):
            self.assertEqual(file.stat().st_mode & 0o777, 0o600)
        # A new process receives an exported staff-specific key on every reload.
        for _ in range(2):
            restored = subprocess.run(
                ['bash', '-c', 'source "$1"; bash -c \'[[ $STAFF_POSTER_ID_KEY = "$1" ]]\' test "$2"',
                 'test', str(shell), KEY], env=self.env, capture_output=True)
            self.assertEqual(restored.returncode, 0)
        self.assertEqual(self.log.read_text().splitlines(), ['database', 'random', 'random', 'database'])
        self.assertNotIn(KEY, self.log.read_text())

    def test_invalid_or_missing_identity_fails_before_database_changes(self):
        cases = [
            '', 'export DATABASE_URL=ignored\n',
            'export POSTER_ID_KEY=\n',
            f'export POSTER_ID_KEY={"0" * 64}\n',
            f'export POSTER_ID_KEY={KEY[:-1]}\n',
            f'export POSTER_ID_KEY={KEY}0\n',
            f'export POSTER_ID_KEY={"g" * 64}\n',
            f'export POSTER_ID_KEY="{KEY}"\n',
            f'export POSTER_ID_KEY={KEY}\nexport POSTER_ID_KEY={KEY}\n',
            f'export POSTER_ID_KEY={KEY}\nPOSTER_ID_KEY=bad\n',
            'export POSTER_ID_KEY=$(touch "$MOCK_LOG")\n',
        ]
        for contents in cases:
            with self.subTest(contents=contents):
                self.database.write_text(contents)
                self.assertNotEqual(self.run_setup().returncode, 0)
                self.assertFalse(self.log.exists())
                self.assertFalse((self.local / 'staff.env').exists())
                self.assertFalse((self.local / 'staff.ps1').exists())
        self.database.unlink()
        self.assertNotEqual(self.run_setup().returncode, 0)
        self.assertFalse(self.log.exists())

    def test_existing_either_file_refuses_overwrite(self):
        self.assertEqual(self.run_setup().returncode, 0)
        originals = {name: (self.local / name).read_bytes()
                     for name in ('staff.env', 'staff.ps1')}
        calls = self.log.read_bytes()
        self.assertNotEqual(self.run_setup().returncode, 0)
        for name, content in originals.items():
            self.assertEqual((self.local / name).read_bytes(), content)
        for surviving in originals:
            for name, content in originals.items():
                (self.local / name).write_bytes(content)
            missing = next(name for name in originals if name != surviving)
            (self.local / missing).unlink()
            self.assertNotEqual(self.run_setup().returncode, 0)
            self.assertEqual((self.local / surviving).read_bytes(), originals[surviving])
            self.assertFalse((self.local / missing).exists())
        self.assertEqual(self.log.read_bytes(), calls)

    def test_dangling_destination_symlink_is_not_overwritten(self):
        for name in ('staff.env', 'staff.ps1'):
            destination = self.local / name
            destination.symlink_to(self.root / 'absent')
            self.assertNotEqual(self.run_setup().returncode, 0)
            self.assertTrue(destination.is_symlink())
            self.assertFalse(self.log.exists())
            destination.unlink()


if __name__ == '__main__':
    unittest.main()
