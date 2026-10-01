"""Browser failure locations never publish captured capabilities or page data."""
import subprocess
import sys
import unittest

from public_upload_fixture import finish_browser


class BrowserDiagnostics(unittest.TestCase):
    def process(self, stderr, code=1):
        return subprocess.Popen([sys.executable, '-c',
                                 'import sys; sys.stderr.buffer.write(bytes.fromhex(sys.argv[1])); sys.exit(int(sys.argv[2]))',
                                 stderr.hex(), str(code)], stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def test_failure_retains_location_without_captured_page_or_capability(self):
        secret = b'a' * 64
        process = self.process(b'Error: http://owned.invalid/upload?capability=' + secret
                               + b'\n at file:///owned/tests/browser/public-upload.mjs:254:10\n')
        with self.assertRaisesRegex(AssertionError, r'^owned upload browser rejected at public-upload\.mjs:254:10$'):
            finish_browser(process, 'public-upload.mjs')

    def test_other_module_and_missing_location_emit_no_error_body(self):
        for stderr in (b'sensitive response', b'at file:///owned/other.mjs:12:3\n',
                       b'at file:///owned/quick-reply-uploadXmjs:12:3\n',
                       b'at file:///owned/quick-reply-upload.mjs:1234567:3\n'):
            with self.subTest(stderr=stderr):
                with self.assertRaisesRegex(AssertionError, r'^owned upload browser rejected at quick-reply-upload\.mjs$'):
                    finish_browser(self.process(stderr), 'quick-reply-upload.mjs')

    def test_success_returns_the_original_protocol_output(self):
        self.assertEqual(finish_browser(self.process(b'private warning', 0), 'public-upload.mjs'), b'')

    def test_unknown_diagnostic_script_is_rejected_before_child_access(self):
        with self.assertRaises(AssertionError):
            finish_browser(None, 'private-script.mjs')


if __name__ == '__main__':
    unittest.main()
