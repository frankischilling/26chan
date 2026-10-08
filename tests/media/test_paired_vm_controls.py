#!/usr/bin/env python3
"""Unprivileged fixture/assertion controls. These do not qualify guest execution."""
import struct
import os
import pathlib
import subprocess
import sys
from unittest import mock
import unittest
import zlib

import paired_vm_fixtures as f


class PairedVmControls(unittest.TestCase):
    def test_optimized_direct_entries_and_imports_fail_before_activity(self):
        directory = pathlib.Path(__file__).parent
        entries = [('test_paired_vm.py', []),
                   ('test_paired_vm.py', ['--mutated-disk', 'padding', '/missing/config', '/missing/source', '/missing/output']),
                   ('test_paired_dispatch_vm.py', []),
                   ('test_paired_dispatch_vm.py', ['--unix-client', '/missing/socket', '/missing/source', '/missing/output', 'eof'])]
        environment = {'PATH': os.environ.get('PATH', ''), 'APP_ENV': 'development',
                       'MEDIA_PAIRED_VM_QUALIFY': '1', 'MEDIA_PAIRED_VM_UNIX_CLIENT': '1'}
        for optimization in ('-O', '-OO'):
            for script, arguments in entries:
                with self.subTest(optimization=optimization, script=script, arguments=arguments):
                    result = subprocess.run([sys.executable, optimization, str(directory / script), *arguments],
                                            env=environment, capture_output=True, text=True, timeout=5)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn('optimized Python cannot run qualification', result.stderr)
                    self.assertNotIn('PASS', result.stdout)
            for module in ('test_paired_vm', 'test_paired_dispatch_vm'):
                result = subprocess.run([sys.executable, optimization, '-c', 'import ' + module],
                                        cwd=directory, env=environment, capture_output=True, text=True, timeout=5)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('optimized Python cannot run qualification', result.stderr)

    def test_coordinator_requires_exactly_one_executed_success(self):
        from test_paired_dispatch_vm import check_coordinator_completion
        success = b'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n'
        check_coordinator_completion(0, success)
        for output in (b'', success.replace(b'1 passed', b'0 passed'),
                       success.replace(b'1 passed', b'11 passed'),
                       success.replace(b'0 ignored', b'1 ignored'),
                       success.replace(b'0 failed', b'1 failed'), success + success):
            with self.assertRaises(RuntimeError):
                check_coordinator_completion(0, output)
        with self.assertRaises(RuntimeError):
            check_coordinator_completion(1, success)

    def test_direct_entries_require_explicit_development_and_opt_in(self):
        import test_paired_vm as vm
        import test_paired_dispatch_vm as dispatch
        invocations = [vm.PairedVmTest.setUpClass,
                       lambda: vm.PairedVmTest().execute(b'unused'),
                       lambda: vm.mutated_disk_main('padding', '/missing', '/missing', '/missing'),
                       dispatch.PairedExercise,
                       lambda: dispatch.PairedExercise.__new__(dispatch.PairedExercise).qualify_coordinator(),
                       lambda: dispatch.unix_client('/missing', '/missing', '/missing', 'eof')]
        # Mock identity only; every call must stop before config, file, SQL,
        # service or socket work. No privileged subprocess is launched.
        for environment, message in [({'APP_ENV': 'production', 'MEDIA_PAIRED_VM_QUALIFY': '1'}, 'APP_ENV'),
                                     ({'MEDIA_PAIRED_VM_QUALIFY': '1'}, 'APP_ENV'),
                                     ({'APP_ENV': 'development'}, 'MEDIA_PAIRED_VM_QUALIFY'),
                                     ({'APP_ENV': 'development', 'MEDIA_PAIRED_VM_QUALIFY': 'true'}, 'MEDIA_PAIRED_VM_QUALIFY')]:
            with mock.patch.dict(os.environ, environment, clear=True), \
                 mock.patch.object(os, 'getuid', return_value=0), \
                 mock.patch.object(os, 'geteuid', return_value=0):
                for invoke in invocations:
                    with self.subTest(environment=environment, invoke=invoke), self.assertRaisesRegex(RuntimeError, message):
                        invoke()
        with mock.patch.dict(os.environ, {'APP_ENV': 'development', 'MEDIA_PAIRED_VM_QUALIFY': '1'}, clear=True), \
             mock.patch.object(os, 'getuid', return_value=1000), \
             mock.patch.object(os, 'geteuid', return_value=1000):
            with self.assertRaisesRegex(RuntimeError, 'identity'):
                f.qualification_guard()
            with self.assertRaisesRegex(RuntimeError, 'Unix-client mode'):
                dispatch.unix_client('/missing', '/missing', '/missing', 'eof')

    def test_exact_caps_preserve_png_and_recorded_replay(self):
        image, replay = f.png(size=f.COMPONENT_BYTES), f.maximum_replay()
        self.assertEqual(len(image), f.COMPONENT_BYTES)
        self.assertEqual(len(replay), f.COMPONENT_BYTES)
        inflater = zlib.decompressobj(-15)
        self.assertEqual(inflater.decompress(replay[12:]) + inflater.flush(),
                         zlib.decompress(f.replay()[12:], -15))
        self.assertTrue(inflater.eof)
        self.assertEqual(inflater.unused_data, b'')
        self.assertEqual(replay[:12], f.replay()[:12])
        offset = 8
        kinds = []
        while offset < len(image):
            length = int.from_bytes(image[offset:offset + 4], 'big')
            kind, body = image[offset + 4:offset + 8], image[offset + 8:offset + 8 + length]
            self.assertEqual(int.from_bytes(image[offset + 8 + length:offset + 12 + length], 'big'),
                             zlib.crc32(kind + body))
            if kind == b'IDAT':
                self.assertEqual(zlib.decompress(body), b'\0\xff\0\0\xff')
            kinds.append(kind)
            offset += length + 12
        self.assertEqual(offset, len(image))
        self.assertEqual(kinds, [b'IHDR', b'IDAT', b'vpAg', b'IEND'])
        request = f.request(f.frame(image, replay))
        self.assertEqual(len(request), 48 + 16_777_272)
        self.assertEqual(int.from_bytes(request[8:16], 'big'), 16_777_272)

    def test_mutations_change_only_expected_disk_boundaries(self):
        request = f.request(f.frame(f.png(size=1024)))
        disk = request + bytes(-len(request) % 512)
        self.assertEqual(f.mutate_disk(disk, 'padding'), disk[:-1] + b'\x01')
        self.assertEqual(f.mutate_disk(disk, 'extra-sector'), disk + bytes(512))
        self.assertEqual(f.mutate_disk(disk, 'short-sector'), disk[:512])
        self.assertEqual(f.mutate_disk(disk, 'outer-version'), b'IBJOB001' + disk[8:])
        self.assertEqual(f.mutate_disk(disk, 'declared-length'),
                         disk[:8] + (16_777_273).to_bytes(8, 'big') + disk[16:])
        with self.assertRaises(ValueError):
            f.mutate_disk(bytes(512), 'short-sector')
        with self.assertRaises(ValueError):
            f.mutate_disk(disk, 'unknown')

    def test_independent_expected_result_and_all_sensitive_boundaries(self):
        for name in (None, 'empty', 'commands'):
            replay = b'' if name is None else f.wire(name)
            if name:
                self.assertEqual(replay[24:28], struct.pack('>HH', 640, 480))
            expected = (b'IBRES002' + struct.pack('>HHI', 2, 64, int(name is not None)) + f.BINDING
                        + struct.pack('>QQ', 20, len(replay))
                        + b'IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff' + replay)
            data = expected + bytes(f.RESULT_BYTES - len(expected))
            f.check_result(data, replay_name=name)
            for offset in (0, 8, 10, 12, 15, 16, 47, 48, 55, 56, 63, 64, 75, 79, 80, len(data) - 1):
                with self.subTest(name=name, offset=offset):
                    bad = bytearray(data)
                    bad[offset] ^= 1
                    with self.assertRaises(ValueError):
                        f.check_result(bytes(bad), replay_name=name)
            for bad in (data[:-1], data + b'\0'):
                with self.assertRaises(ValueError):
                    f.check_result(bad, replay_name=name)
            with self.assertRaises(ValueError):
                f.check_result(data, replay_name=name, binding=bytes(32))


if __name__ == '__main__':
    unittest.main()
