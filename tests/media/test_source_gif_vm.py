"""Real isolated GIF-v7 processing using only owned synthetic inputs."""
import os
import pathlib
import signal
import struct
import subprocess
import tempfile
import time
import unittest
from test_still_vm import StillVmTest, OUTPUT_BYTES
from test_vm import REPO, VmTest, check_probe_report


class SourceGifVmTest(unittest.TestCase):
    input_kind = 'gif-v7'
    run_job = StillVmTest.run_job

    def policy(self, thread=True):
        bound = 250 if thread else 125
        return b'IBGIF007' + bytes([4 if thread else 0, 3]) + struct.pack('>HH', bound, bound) + bytes(2)

    def retained(self, output, policy):
        self.assertEqual(output[:8], b'IBIMG007')
        self.assertEqual(output[8:24], policy)
        self.assertEqual(output[40:48], bytes(8))
        length, thumbnail, mobile = struct.unpack('>III', output[48:60])
        self.assertEqual(mobile, 0)
        self.assertFalse(any(output[60:64]))
        image = output[64:64 + length]
        thumb = output[64 + length:64 + length + thumbnail]
        self.assertTrue(thumb.startswith(b'\xff\xd8\xff') and thumb.endswith(b'\xff\xd9'))
        self.assertFalse(any(output[64 + length + thumbnail:]))
        return image

    def test_unchanged_animation_and_configured_cleanup_match_pinned_reference(self):
        for name in ('static', 'transparent', 'interlaced', 'animated', 'partial'):
            original = (REPO / 'tests/media/fixtures/gif' / f'{name}.gif').read_bytes()
            with self.subTest(name=name):
                policy = self.policy()
                self.assertEqual(self.retained(self.run_job(policy + original), policy), original)
                # A comment before the first frame requires source cleanup.
                table = 3 * (2 << (original[10] & 7)) if original[10] & 128 else 0
                at = 13 + table
                tagged = original[:at] + b'!\xfe\x0ePRIVATE-MARKER\x00' + original[at:]
                with tempfile.TemporaryDirectory(prefix='gif-reference-') as directory:
                    source, target = pathlib.Path(directory) / 'in.gif', pathlib.Path(directory) / 'out.gif'
                    source.write_bytes(tagged)
                    subprocess.run(['/usr/lib/26chan-gif/gifsicle', '--no-comments', '--no-extensions',
                                    str(source), '-o', str(target)], check=True, timeout=5,
                                   env={'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}, capture_output=True)
                    cleaned = self.retained(self.run_job(policy + tagged), policy)
                    self.assertEqual(cleaned, target.read_bytes())
                    self.assertNotIn(b'PRIVATE-MARKER', cleaned)

    def test_large_canvas_and_malformed_inputs_remain_bounded(self):
        original = (REPO / 'tests/media/fixtures/gif/static.gif').read_bytes()
        large = original[:6] + struct.pack('<HH', 10000, 10000) + original[10:]
        policy = self.policy(False)
        output = self.run_job(policy + large, measure=True)
        self.assertEqual(self.retained(output, policy), large)
        self.assertEqual(struct.unpack('>II', output[24:32]), (10000, 10000))
        self.assertEqual(struct.unpack('>II', output[32:40]), (125, 125))
        for invalid in (b'not a GIF', original[:-1], original + b'extra;',
                        (REPO / 'tests/media/fixtures/gif/too-wide.gif').read_bytes(),
                        original[:6] + struct.pack('<HH', 10001, 1) + original[10:]):
            self.assertEqual(self.run_job(policy + invalid), bytes(OUTPUT_BYTES))
        self.assertEqual(self.retained(self.run_job(policy + original), policy), original)

    def test_live_profile_and_containment(self):
        VmTest().check_live_vmm_limits('gif-v7', 2560 * 1024 * 1024)
        for mode, checks in [('inspect', 9), ('limits', 4), ('memory', 1), ('process', 1), ('disk', 1)]:
            output = self.run_job(mode.encode().ljust(17, b' '), probe=True)
            check_probe_report(output[:4_194_816], checks)
            self.assertFalse(any(output[4_194_816:]))

    def test_deadline_and_cancellation_remove_the_entire_job(self):
        start = time.monotonic()
        self.run_job(b'sleep'.ljust(17, b' '), probe=True, expect_failure=True)
        self.assertGreaterEqual(time.monotonic() - start, 40)
        self.assertLess(time.monotonic() - start, 70)
        vm = VmTest()
        for signum in (signal.SIGTERM, signal.SIGINT):
            with vm.sleeping_vm('gif-v7') as (runner, unit, workspace, disk, vmm):
                runner.send_signal(signum)
                runner.communicate(timeout=10)
                self.assertNotEqual(runner.returncode, 0)
                self.assertFalse(disk.exists())
                self.assertFalse(workspace.exists())
                vm.assert_clean()


if __name__ == '__main__':
    if os.geteuid() != 0 or 'MEDIA_VM_TEST_CONFIG' not in os.environ:
        raise SystemExit('requires an owned root VM qualification environment')
    unittest.main()
