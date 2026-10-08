#!/usr/bin/env python3
"""Unprivileged controls; mocks do not qualify broker or VM execution."""
import ast
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import paired_unix_client as client
import paired_vm_fixtures as f
import test_paired_dispatch_vm as dispatch


class PairedUnixClientControls(unittest.TestCase):
    def test_constant_and_standard_library_only(self):
        self.assertEqual(client.RESULT_BYTES, f.RESULT_BYTES)
        tree = ast.parse(pathlib.Path(client.__file__).read_text())
        imports = [node for node in ast.walk(tree) if isinstance(node, (ast.Import, ast.ImportFrom))]
        self.assertTrue(all(isinstance(node, ast.Import) for node in imports))
        self.assertEqual({alias.name for node in imports for alias in node.names},
                         {'os', 'pathlib', 'pwd', 're', 'socket', 'stat', 'sys'})

    def test_standalone_process_without_source_checkout(self):
        # Removing this temporary source checkout is an equivalent import-access
        # boundary, not evidence of real root/gateway/socket execution.
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            source, staged = root / 'checkout', root / 'bin'
            source.mkdir()
            staged.mkdir()
            original = source / 'paired_unix_client.py'
            original.write_bytes(pathlib.Path(client.__file__).read_bytes())
            target = staged / original.name
            shutil.copyfile(original, target)
            shutil.rmtree(source)
            environment = {'PATH': '/usr/bin:/bin', 'APP_ENV': 'development',
                           'MEDIA_PAIRED_VM_QUALIFY': '1', 'MEDIA_PAIRED_VM_UNIX_CLIENT': '1'}
            for optimization in (None, '-O', '-OO'):
                command = [sys.executable, '-I']
                if optimization:
                    command.append(optimization)
                result = subprocess.run([*command, str(target), '/missing', '/missing', '/missing', 'eof'],
                                        cwd=root, env=environment, capture_output=True, timeout=5)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(result.stdout, b'')
                if optimization or os.geteuid() == 0:
                    self.assertEqual(result.stderr, b'PAIRED_UNIX_CLIENT stage=guard error=RuntimeError errno=0\n')
                else:
                    self.assertRegex(result.stderr, rb'^PAIRED_UNIX_CLIENT stage=(identity|paths) error=(KeyError|RuntimeError) errno=0\n$')
                self.assertNotIn(str(root).encode(), result.stderr)

    def test_diagnostic_grammar_and_no_untrusted_content(self):
        for error in (RuntimeError('secret /private/path'), OSError(13, 'secret /private/path'),
                      type('UserControlledSecret', (Exception,), {})('secret')):
            value = client.diagnostic('receive', error)
            self.assertNotIn('secret', value)
            with self.assertRaisesRegex(RuntimeError, r'^paired Unix client failed: stage=receive error='):
                dispatch.check_client_completion(1, b'', value.encode())
        self.assertEqual(client.diagnostic('secret', OSError(99999, 'secret')),
                         'PAIRED_UNIX_CLIENT stage=arguments error=OSError errno=0\n')
        valid = b'PAIRED_UNIX_CLIENT stage=connect error=PermissionError errno=13\n'
        for error in (b'', valid + b'secret', valid.replace(b'13', b'4096'),
                      valid.replace(b'13', b'0013'), valid.replace(b'connect', b'secret'),
                      valid.replace(b'PermissionError', b'secret'), b'secret' * 100):
            with self.assertRaisesRegex(RuntimeError, '^paired Unix client failed: diagnostic unavailable$'):
                dispatch.check_client_completion(1, b'', error)
        with self.assertRaisesRegex(RuntimeError, 'diagnostic unavailable'):
            dispatch.check_client_completion(1, b'secret', valid)
        with self.assertRaisesRegex(RuntimeError, 'diagnostic unavailable'):
            dispatch.check_client_completion(0, b'', valid)
        dispatch.check_client_completion(0, b'', b'')

    def test_staging_path_identity_permissions_and_exclusive_creation(self):
        root = pathlib.Path('/run/26chan-dispatch-abcdefgh')
        directory = root / 'bin'
        payload = pathlib.Path(client.__file__).read_bytes()
        directory_info = SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_uid=0, st_gid=0)
        file_info = SimpleNamespace(st_mode=stat.S_IFREG | 0o644, st_uid=0, st_gid=0, st_size=len(payload))
        with mock.patch.object(f, 'qualification_guard') as guard, \
             mock.patch.object(pathlib.Path, 'lstat', return_value=directory_info), \
             mock.patch.object(os, 'open', return_value=9) as opened, \
             mock.patch.object(os, 'fdopen') as fdopen, \
             mock.patch.object(os, 'fchown') as chown, \
             mock.patch.object(os, 'fchmod') as chmod, \
             mock.patch.object(os, 'fstat', return_value=file_info):
            stream = fdopen.return_value.__enter__.return_value
            stream.fileno.return_value = 9
            self.assertEqual(dispatch.stage_unix_client(root, directory), directory / 'paired-unix-client.py')
            guard.assert_called_once_with()
            opened.assert_called_once_with(directory / 'paired-unix-client.py',
                                           os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            stream.write.assert_called_once_with(payload)
            chown.assert_called_once_with(9, 0, 0)
            chmod.assert_called_once_with(9, 0o644)
        for invalid in ('/tmp/26chan-dispatch-abcdefgh', '/run/26chan-dispatch-short', '/run/other-abcdefgh'):
            with mock.patch.object(f, 'qualification_guard'), mock.patch.object(os, 'open') as opened:
                with self.assertRaisesRegex(RuntimeError, 'staging path'):
                    dispatch.stage_unix_client(invalid, pathlib.Path(invalid) / 'bin')
                opened.assert_not_called()
        for changes in ({'st_uid': 1000}, {'st_gid': 1000}, {'st_mode': stat.S_IFDIR | 0o777},
                        {'st_mode': stat.S_IFLNK | 0o755}):
            info = SimpleNamespace(**{**vars(directory_info), **changes})
            with mock.patch.object(f, 'qualification_guard'), \
                 mock.patch.object(pathlib.Path, 'lstat', return_value=info), \
                 mock.patch.object(os, 'open') as opened:
                with self.assertRaisesRegex(RuntimeError, 'staging directory'):
                    dispatch.stage_unix_client(root, directory)
                opened.assert_not_called()

    def test_client_identity_and_path_guards_before_file_activity(self):
        environment = {'APP_ENV': 'development', 'MEDIA_PAIRED_VM_QUALIFY': '1',
                       'MEDIA_PAIRED_VM_UNIX_CLIENT': '1'}
        gateway = SimpleNamespace(pw_uid=1234, pw_gid=2345)
        root = pathlib.Path('/run/26chan-dispatch-abcdefgh')
        paths = (root / 'broker/broker.sock', root / 'gateway/request.bin', root / 'gateway/candidate.bin')
        with mock.patch.dict(os.environ, environment, clear=True), \
             mock.patch.object(os, 'getuid', return_value=1234), \
             mock.patch.object(os, 'geteuid', return_value=1234), \
             mock.patch.object(os, 'getgid', return_value=2345), \
             mock.patch.object(os, 'getegid', return_value=2345), \
             mock.patch.object(client.pwd, 'getpwnam', return_value=gateway), \
             mock.patch.object(pathlib.Path, 'lstat') as lstat, \
             mock.patch.object(os, 'open') as opened:
            for name in ('getuid', 'geteuid', 'getgid', 'getegid'):
                with mock.patch.object(os, name, return_value=9876):
                    with self.assertRaisesRegex(RuntimeError, 'identity'):
                        client.unix_client(*paths, 'eof')
            for index in range(3):
                invalid = list(paths)
                invalid[index] = pathlib.Path('/missing')
                with self.assertRaisesRegex(RuntimeError, 'paths'):
                    client.unix_client(*invalid, 'eof')
            with self.assertRaisesRegex(RuntimeError, 'Unix-client mode'):
                client.unix_client(*paths, 'unknown')
            lstat.assert_not_called()
            opened.assert_not_called()

    def test_exchange_uses_staged_isolated_client_with_clear_environment(self):
        exercise = dispatch.PairedExercise.__new__(dispatch.PairedExercise)
        exercise.keys = mock.MagicMock()
        exercise.broker_dir = pathlib.Path('/run/26chan-dispatch-abcdefgh/broker')
        exercise.unix_client_path = pathlib.Path('/run/26chan-dispatch-abcdefgh/bin/paired-unix-client.py')
        exercise.gateway = object()
        exercise.write = mock.Mock()
        exercise.clean_vm = mock.Mock()
        process = mock.Mock(returncode=0)
        process.communicate.return_value = (b'', b'')
        exercise.launch = mock.Mock(return_value=process)
        exercise.keys.__truediv__.return_value.read_bytes.return_value = b''
        with mock.patch.object(f, 'qualification_guard'):
            exercise.exchange(b'payload', rejected=True)
        arguments, identity, environment = exercise.launch.call_args.args
        self.assertEqual(arguments[:3], [sys.executable, '-I', str(exercise.unix_client_path)])
        self.assertIs(identity, exercise.gateway)
        self.assertEqual(environment, {**dispatch.SAFE, 'MEDIA_PAIRED_VM_QUALIFY': '1',
                                       'MEDIA_PAIRED_VM_UNIX_CLIENT': '1'})


if __name__ == '__main__':
    unittest.main()
