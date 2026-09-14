from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class SignalCleanupTests(unittest.TestCase):
    def check_cleanup(self, monitoring, delivery):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "state"
            code = """
from contextlib import ExitStack
import pathlib, signal, sys, tempfile
sys.path.insert(0, sys.argv[1])
import qualify
qualify.install_signal_cleanup()
class Finalizer:
    def __del__(self):
        signal.raise_signal(signal.SIGTERM)
def predicate():
    if sys.argv[3] != 'direct':
        finalizer = Finalizer()
        del finalizer
    if sys.argv[3] == 'finalizer-error':
        raise OSError('synthetic retryable error')
    return sys.argv[3] != 'finalizer-false'
def cleanup():
    # A second signal must not interrupt registered cleanup callbacks.
    signal.raise_signal(signal.SIGTERM)
    pathlib.Path(sys.argv[2] + '.cleaned').write_text('cleaned')
with tempfile.TemporaryDirectory(prefix='board-signal-test-') as work, ExitStack() as stack:
    stack.callback(cleanup)
    pathlib.Path(sys.argv[2]).write_text(work)
    (pathlib.Path(work) / 'test.token').write_text('synthetic')
    if sys.argv[3] == 'direct':
        signal.raise_signal(signal.SIGTERM)
    qualify.wait_for('signal checkpoint', predicate, [], seconds=0.2)
"""
            result = subprocess.run([sys.executable, "-c", code, str(monitoring), str(marker), delivery],
                                    capture_output=True, text=True, timeout=15)
            self.assertEqual(result.returncode, 143, result.stderr)
            self.assertEqual(result.stderr, "", "Signal escaped through a finalizer")
            self.assertEqual(Path(str(marker) + '.cleaned').read_text(), 'cleaned')
            self.assertFalse(Path(marker.read_text()).exists(), "Temporary credentials survived SIGTERM")

    def test_sigterm_unwinds_at_polling_checkpoint(self):
        monitoring = Path(__file__).resolve().parent
        for qualifier in (monitoring, monitoring / 'authenticated'):
            for delivery in ('direct', 'finalizer-true', 'finalizer-false', 'finalizer-error'):
                with self.subTest(qualifier=qualifier.name, delivery=delivery):
                    self.check_cleanup(qualifier, delivery)


if __name__ == "__main__":
    unittest.main()
