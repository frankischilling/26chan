"""The operator runner refuses unrelated identity keys before job setup."""
import contextlib
import importlib.util
import io
import pathlib
import sys
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[2]


class IdentityEnvironmentTest(unittest.TestCase):
    def test_keys_are_rejected_before_configuration_or_job_execution(self):
        sys.path.insert(0, str(ROOT / 'scripts/media'))
        self.addCleanup(lambda: sys.path.remove(str(ROOT / 'scripts/media')))
        spec = importlib.util.spec_from_file_location('identity_runner', ROOT / 'scripts/media/run-job.py')
        runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(runner)
        argv = ['run-job.py', 'owned-config', 'owned-input', 'owned-output']
        with mock.patch.object(sys, 'argv', argv), mock.patch.object(runner.os, 'geteuid', return_value=0), \
                mock.patch.object(runner.signal, 'signal'), mock.patch.object(runner, 'configuration', return_value={'owned': True}) as config, \
                mock.patch.object(runner, 'run') as run:
            # Healthy control reaches the identical job path with no unrelated key.
            with mock.patch.dict(runner.os.environ, {}, clear=True):
                runner.main()
            config.assert_called_once_with('owned-config')
            run.assert_called_once_with({'owned': True}, 'owned-input', 'owned-output')
            for variable in ['POSTER_ID_KEY', 'TRIPCODE_KEY', 'STAFF_TRIPCODE_KEY', 'STAFF_POSTER_ID_KEY', 'AWS_ACCESS_KEY', 'OWNED_API_KEY']:
                config.reset_mock(); run.reset_mock()
                error = io.StringIO()
                with mock.patch.dict(runner.os.environ, {variable: 'owned-private-key'}, clear=True), \
                        contextlib.redirect_stderr(error), self.assertRaises(SystemExit) as stopped:
                    runner.main()
                self.assertEqual(stopped.exception.code, 2)
                self.assertIn('remove credential-bearing environment variables', error.getvalue())
                self.assertNotIn('owned-private-key', error.getvalue())
                config.assert_not_called(); run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
