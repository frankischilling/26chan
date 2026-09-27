"""Exercise the real Unix control socket with startup deliberately paused."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from resource_fixture import OwnedFixture, MEMORY_LIMIT, TASK_LIMIT, wait_for


@unittest.skipUnless(sys.platform.startswith('linux'), 'Owned resource controls use Linux Unix sockets')
class ResourceControlTests(unittest.TestCase):
    def test_control_path_is_published_only_after_listen_and_removed_on_stop(self):
        # No mount, service, pressure command or real cgroup is used here. The
        # unchanged serve/command functions exchange a harmless ping over a real
        # socket; a gate holds startup exactly between bind and listen.
        with tempfile.TemporaryDirectory(prefix='br-', dir='/tmp') as directory:
            root = Path(directory) / 'board-resource-monitor-0123456789abcdef'
            root.mkdir(mode=0o700)
            fixture = OwnedFixture(root)
            fixture.private.mkdir(mode=0o700)
            cgroup = root / 'synthetic-ceilings'
            cgroup.mkdir()
            (cgroup / 'memory.max').write_text(str(MEMORY_LIMIT))
            (cgroup / 'pids.max').write_text(str(TASK_LIMIT))
            bound, proceed = root / 'bound', root / 'proceed'
            runner = root / 'paused_socket.py'
            runner.write_text('''import pathlib, socket, sys, time
sys.path.insert(0, sys.argv[1])
import resource_fixture
bound, proceed = pathlib.Path(sys.argv[4]), pathlib.Path(sys.argv[5])
class PausedSocket(socket.socket):
    def listen(self, backlog):
        bound.touch()
        deadline = time.monotonic() + 5
        while not proceed.exists():
            if time.monotonic() >= deadline:
                raise AssertionError('Synthetic listen gate was not released')
            time.sleep(0.01)
        return super().listen(backlog)
resource_fixture.socket.socket = PausedSocket
resource_fixture.serve(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]))
''')
            child = subprocess.Popen([sys.executable, str(runner), str(Path(__file__).resolve().parent),
                                      str(fixture.control), str(cgroup), str(bound), str(proceed)],
                                     stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                wait_for(bound.exists, 3)
                self.assertIsNone(child.poll())
                if fixture.control.exists():
                    with self.assertRaises(ConnectionRefusedError):
                        fixture.command('ping')
                self.assertFalse(fixture.control.exists(), 'A bound socket is not yet a listening control')
                proceed.touch()
                wait_for(fixture.control.exists, 3)
                self.assertEqual(fixture.control.stat().st_mode & 0o777, 0o600)
                self.assertEqual(list(fixture.private.iterdir()), [fixture.control])
                self.assertEqual(fixture.command('ping'), {'ok': True})
                self.assertEqual(fixture.command('ping'), {'ok': True})
            finally:
                if child.poll() is None:
                    child.terminate()
                output, error = child.communicate(timeout=5)
            self.assertEqual(child.returncode, 143, error.decode(errors='replace'))
            self.assertEqual(output, b'')
            self.assertEqual(error, b'')
            self.assertFalse(fixture.control.exists())
            self.assertEqual(list(fixture.private.iterdir()), [])

    def test_failed_startup_removes_the_unpublished_socket(self):
        # Serve must also unwind a failure before readiness becomes visible.
        # Reuse a real bound socket and force listen to fail in the child.
        with tempfile.TemporaryDirectory(prefix='br-', dir='/tmp') as directory:
            root = Path(directory)
            (root / 'memory.max').write_text(str(MEMORY_LIMIT))
            (root / 'pids.max').write_text(str(TASK_LIMIT))
            runner = root / 'failed_listen.py'
            runner.write_text('''import pathlib, socket, sys
sys.path.insert(0, sys.argv[1])
import resource_fixture
class FailedSocket(socket.socket):
    def listen(self, backlog):
        raise RuntimeError('synthetic listen failure')
resource_fixture.socket.socket = FailedSocket
try:
    resource_fixture.serve(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]))
except RuntimeError as error:
    assert str(error) == 'synthetic listen failure'
else:
    raise AssertionError('A failed listener was accepted')
''')
            control = root / 'fixture.sock'
            result = subprocess.run([sys.executable, str(runner), str(Path(__file__).resolve().parent),
                                     str(control), str(root)], capture_output=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            self.assertEqual(result.stdout, b'')
            self.assertEqual(result.stderr, b'')
            self.assertFalse(control.exists())
            self.assertFalse(any(path.is_socket() for path in root.iterdir()))


if __name__ == '__main__':
    unittest.main()
