#!/usr/bin/env python3
"""Check bootstrap environment output with mocked PostgreSQL and randomness."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).with_name('dev-db.sh').read_text()


class DevelopmentEnvironmentTest(unittest.TestCase):
    def test_identity_is_private_persisted_and_not_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / 'scripts'
            scripts.mkdir()
            binaries = root / 'bin'
            binaries.mkdir()
            # Only replace the fixed PostgreSQL installation path in the copy.
            script = scripts / 'dev-db.sh'
            script.write_text(SOURCE.replace('/usr/lib/postgresql/16/bin', str(binaries)))
            mocks = {
                'initdb': 'exit 0',
                'id': 'printf "0\\n"',
                'chown': 'exit 0',
                'runuser': 'cat >/dev/null; exit 0',
                'openssl': '''[[ $1 = rand && $2 = -hex ]] || exit 1
printf '%s\\n' "$3" >> "$MOCK_RANDOM_LOG"
printf '%*s\\n' "$((2 * $3))" '' | tr ' ' a''',
                'mktemp': '''if [[ ${1:-} = -d ]]; then
  mkdir "$MOCK_ROOT/cluster"
  printf '%s\\n' "$MOCK_ROOT/cluster"
else
  touch "$MOCK_ROOT/password"
  printf '%s\\n' "$MOCK_ROOT/password"
fi''',
            }
            for name, body in mocks.items():
                executable = binaries / name
                executable.write_text('#!/usr/bin/env bash\nset -euo pipefail\n' + body + '\n')
                executable.chmod(0o700)
            env = dict(os.environ, PATH=f'{binaries}:{os.environ["PATH"]}',
                       MOCK_ROOT=str(root), MOCK_RANDOM_LOG=str(root / 'random.log'))
            first = subprocess.run(['bash', str(script)], env=env, input='',
                                   capture_output=True, text=True, check=True)
            shell_file = root / '.local/database.env'
            powershell_file = root / '.local/database.ps1'
            shell = shell_file.read_text()
            powershell = powershell_file.read_text()
            key = re.search(r'^export POSTER_ID_KEY=([a-f0-9]{64})$', shell, re.M)
            self.assertIsNotNone(key)
            self.assertIn(f"$env:POSTER_ID_KEY = '{key[1]}'", powershell)
            self.assertEqual((root / 'random.log').read_text().splitlines(), ['24', '24', '24', '32'])
            for output in (first.stdout, first.stderr):
                self.assertNotIn(key[1], output)
                self.assertNotIn('a' * 48, output)
            for file in (shell_file, powershell_file):
                self.assertEqual(file.stat().st_mode & 0o777, 0o600)
            # Sourcing the generated shell file restores the same key.
            subprocess.run(['bash', '-c', 'source "$1"; [[ $POSTER_ID_KEY = "$2" ]]',
                            'test', str(shell_file), key[1]], env=env, check=True)
            second = subprocess.run(['bash', str(script)], env=env, input='', capture_output=True)
            self.assertNotEqual(second.returncode, 0)
            self.assertEqual(shell_file.read_text(), shell)
            self.assertEqual(powershell_file.read_text(), powershell)
            # A surviving PowerShell environment also prevents regeneration.
            shell_file.unlink()
            third = subprocess.run(['bash', str(script)], env=env, input='', capture_output=True)
            self.assertNotEqual(third.returncode, 0)
            self.assertEqual(powershell_file.read_text(), powershell)
            self.assertEqual((root / 'random.log').read_text().splitlines(), ['24', '24', '24', '32'])
            self.assertFalse((root / 'password').exists())


if __name__ == '__main__':
    unittest.main()
