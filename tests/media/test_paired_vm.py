#!/usr/bin/env python3
"""Explicit owned-root qualification. Every candidate comes from real Firecracker.

Run separately from unit discovery, after provision-test.py. No approval occurs.
The mutated-disk helper changes only input bytes after the real runner's framing;
it does not replace VM execution, resource policy, init, decoder, or collection.
"""
import importlib.util
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import unittest

if sys.flags.optimize:
    raise RuntimeError('optimized Python cannot run qualification')

from owned_process import run_owned
import paired_vm_fixtures as f
import test_vm

SAFE = {'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'APP_ENV': 'development'}


class PairedVmTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        f.qualification_guard()
        cls.config = os.environ['MEDIA_VM_TEST_CONFIG']

    def execute(self, payload, *, mutation=None, rejected=False, runner_rejected=False, **expected):
        f.qualification_guard()
        with tempfile.TemporaryDirectory(prefix='26chan-paired-vm-') as name:
            root = pathlib.Path(name)
            source, destination = root / 'request', root / 'result'
            source.write_bytes(payload)
            if mutation:
                command = [sys.executable, __file__, '--mutated-disk', mutation, self.config,
                           str(source), str(destination)]
            else:
                command = [sys.executable, str(f.REPO / 'scripts/media/run-job.py'),
                           '--input-kind', 'paired-v2', self.config, str(source), str(destination)]
            result = run_owned(command, env={**SAFE, 'MEDIA_PAIRED_VM_QUALIFY': '1'}, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, timeout=35, post_check=test_vm.VmTest().assert_clean)
            test_vm.VmTest().assert_clean()
            if runner_rejected:
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(destination.exists())
            else:
                self.assertEqual(result.returncode, 0, result.stderr)
                data = destination.read_bytes()
                if rejected:
                    self.assertEqual(data, bytes(f.RESULT_BYTES), 'invalid input produced candidate bytes')
                else:
                    f.check_result(data, **expected)

    def test_png_only_and_recorded_replays_have_exact_independent_results(self):
        for replay_name in (None, 'empty', 'commands'):
            for width, height in ((1, 1), (3, 2)):
                with self.subTest(replay=replay_name, width=width, height=height):
                    replay = None if replay_name is None else f.replay(replay_name)
                    binding = os.urandom(32)
                    self.execute(f.request(f.frame(f.png(width, height), replay), binding),
                                 binding=binding, width=width, height=height, replay_name=replay_name)

    def test_combined_exact_input_cap_decodes_under_unchanged_limits(self):
        body = f.frame(f.png(size=f.COMPONENT_BYTES), f.maximum_replay())
        self.assertEqual(len(body), 16_777_272)
        self.execute(f.request(body), replay_name='empty')

    def test_maximum_rgba_and_one_over_dimension_boundary(self):
        self.execute(f.request(f.frame(f.png(1024, 1024), f.replay())),
                     width=1024, height=1024, replay_name='empty')
        self.execute(f.request(f.frame(f.png(1025, 1))), rejected=True)

    def test_invalid_components_never_return_a_partial_candidate(self):
        images = [b'not PNG', f.png()[:-1], f.png()[:40],
                  (f.REPO / 'tests/media/fixtures/jpeg/baseline.jpg').read_bytes(),
                  (f.REPO / 'tests/media/fixtures/gif/static.gif').read_bytes()]
        bad_crc = bytearray(f.png())
        bad_crc[29] ^= 1
        images.append(bytes(bad_crc))
        for index, image in enumerate(images):
            with self.subTest(image=index):
                self.execute(f.request(f.frame(image, f.replay())), rejected=True)
        replays = [b'invalid TGKR', f.replay()[:-1], f.replay() + b'\0',
                   f.replay()[:4] + (18 * 1024 * 1024 + 1).to_bytes(4, 'big') + f.replay()[8:]]
        for index, replay in enumerate(replays):
            with self.subTest(replay=index):
                self.execute(f.request(f.frame(f.png(), replay)), rejected=True)

    def test_guest_rejects_inner_framing_presence_lengths_and_trailer(self):
        valid = f.frame()
        for offset in (0, 8, 10, 12, 15, 23, 31, len(valid) - 1):
            with self.subTest(offset=offset):
                changed = bytearray(valid)
                changed[offset] ^= 1
                self.execute(f.request(bytes(changed)), rejected=True)
        self.execute(f.request(f.frame(f.png(), b'')), rejected=True)
        self.execute(f.request(f.frame(bytes(f.COMPONENT_BYTES + 1))), rejected=True)

    def test_actual_guest_rejects_disk_padding_extension_and_truncation(self):
        for mutation in ('padding', 'extra-sector', 'short-sector', 'outer-version', 'declared-length'):
            with self.subTest(mutation=mutation):
                payload = f.request(f.frame(f.png(size=1024))) if mutation == 'short-sector' else f.request()
                self.execute(payload, mutation=mutation, rejected=True)

    def test_paired_mode_resource_and_deadline_controls(self):
        f.qualification_guard()
        # Separate qualification initramfs: these are green/red containment
        # reports, not decoded candidates or evidence of replay semantics.
        import time
        config = os.environ['MEDIA_VM_PROBE_CONFIG']
        for mode in ('inspect', 'memory', 'process', 'disk', 'cpu', 'sleep'):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory(prefix='26chan-paired-probe-') as name:
                root = pathlib.Path(name)
                source, destination = root / 'request', root / 'result'
                command = mode.encode().ljust(57, b' ')
                source.write_bytes(f.request(command))
                started = time.monotonic()
                result = run_owned([sys.executable, str(f.REPO / 'scripts/media/run-job.py'),
                                    '--input-kind', 'paired-v2', config, str(source), str(destination)],
                                   env=SAFE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, timeout=35, post_check=test_vm.VmTest().assert_clean)
                elapsed = time.monotonic() - started
                test_vm.VmTest().assert_clean()
                if mode == 'sleep':
                    self.assertNotEqual(result.returncode, 0)
                    self.assertFalse(destination.exists())
                    self.assertGreaterEqual(elapsed, 14)
                    self.assertLess(elapsed, 25)
                    continue
                self.assertEqual(result.returncode, 0, result.stderr)
                data = destination.read_bytes()
                self.assertEqual(len(data), f.RESULT_BYTES)
                if mode == 'cpu':
                    self.assertEqual(data, bytes(f.RESULT_BYTES))
                    self.assertGreaterEqual(elapsed, 4)
                    self.assertLess(elapsed, 8)
                else:
                    count = 9 if mode == 'inspect' else 1
                    expected = b'IBRGBA01' + count.to_bytes(4, 'big') + (1).to_bytes(4, 'big') + b'\0\xff\0\xff' * count
                    self.assertEqual(data[:len(expected)], expected)
                    self.assertFalse(any(data[len(expected):]))

    def test_runner_rejects_outer_eof_and_cap_without_fallback(self):
        data = f.request()
        cases = (data[:-1], data + b'\0', b'IBJOB001' + data[8:],
                 b'IBJOB002' + (16_777_273).to_bytes(8, 'big') + data[16:])
        for index, payload in enumerate(cases):
            with self.subTest(index=index):
                self.execute(payload, runner_rejected=True)


def mutated_disk_main(mutation, config, source, destination):
    f.qualification_guard()
    sys.path.insert(0, str(f.REPO / 'scripts/media'))
    spec = importlib.util.spec_from_file_location('paired_vm_runner', f.REPO / 'scripts/media/run-job.py')
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    original = runner.input_disk

    def input_disk(source, target, **kwargs):
        original(source, target, **kwargs)
        pathlib.Path(target).write_bytes(f.mutate_disk(pathlib.Path(target).read_bytes(), mutation))

    runner.input_disk = input_disk
    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, runner.cancel)
    runner.run(runner.configuration(config), source, destination, input_kind='paired-v2')


if __name__ == '__main__':
    f.qualification_guard()
    if len(sys.argv) == 6 and sys.argv[1] == '--mutated-disk':
        mutated_disk_main(*sys.argv[2:])
    else:
        unittest.main()
