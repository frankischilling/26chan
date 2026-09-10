from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class SignalCleanupTests(unittest.TestCase):
    def test_python_delivered_sigterm_unwinds_temporary_credentials(self):
        monitoring = Path(__file__).resolve().parent
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "state"
            code = """
import pathlib, signal, sys, tempfile
sys.path.insert(0, sys.argv[1])
import qualify
qualify.install_signal_cleanup()
with tempfile.TemporaryDirectory(prefix='board-signal-test-') as work:
    pathlib.Path(sys.argv[2]).write_text(work)
    (pathlib.Path(work) / 'test.token').write_text('synthetic')
    signal.raise_signal(signal.SIGTERM)
"""
            result = subprocess.run([sys.executable, "-c", code, str(monitoring), str(marker)],
                                    capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 143, result.stderr)
            self.assertFalse(Path(marker.read_text()).exists(), "Temporary credentials survived SIGTERM")


if __name__ == "__main__":
    unittest.main()
