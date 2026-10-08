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

    def test_http_status_and_allowlisted_category_exclude_response_data(self):
        secret = b'a' * 64
        for status, category in [(b'429', b'plain'), (b'500', b'html'), (b'403', b'json'), (b'408', b'other')]:
            with self.subTest(status=status, category=category):
                error = (b'private response ' + secret + b'\nOWNED_UPLOAD_RESPONSE status='
                         + status + b' type=' + category
                         + b'\n at file:///owned/quick-reply-upload.mjs:65:82\n')
                expected = ('owned upload browser rejected at quick-reply-upload.mjs:65:82'
                            + ' (HTTP ' + status.decode() + ', ' + category.decode() + ')')
                with self.assertRaises(AssertionError) as result:
                    finish_browser(self.process(error), 'quick-reply-upload.mjs')
                self.assertEqual(str(result.exception), expected)

    def test_response_diagnostic_rejects_unbounded_or_unlisted_details(self):
        for marker in (b'OWNED_UPLOAD_RESPONSE status=999 type=plain',
                       b'OWNED_UPLOAD_RESPONSE status=2000 type=json',
                       b'OWNED_UPLOAD_RESPONSE status=500 type=private',
                       b'OWNED_UPLOAD_RESPONSE status=500 type=html secret=value',
                       b'prefix OWNED_UPLOAD_RESPONSE status=500 type=html'):
            with self.subTest(marker=marker):
                with self.assertRaisesRegex(AssertionError, r'^owned upload browser rejected at quick-reply-upload\.mjs$'):
                    finish_browser(self.process(marker + b'\n'), 'quick-reply-upload.mjs')

    def test_read_failures_identify_stage_without_leaking_response_details(self):
        for stage in (b'upload', b'post'):
            for failure in (b'http', b'json', b'body'):
                error = (b'private url and capability\nOWNED_UPLOAD_RESPONSE status=200 type=json stage='
                         + stage + b' failure=' + failure + b'\n')
                with self.subTest(stage=stage, failure=failure):
                    with self.assertRaises(AssertionError) as result:
                        finish_browser(self.process(error), 'quick-reply-upload.mjs')
                    self.assertEqual(str(result.exception), 'owned upload browser rejected at quick-reply-upload.mjs'
                                     + f' (HTTP 200, json, {stage.decode()}, {failure.decode()})')

    def test_stage_classifications_reject_extra_or_unlisted_details(self):
        for marker in (b'OWNED_UPLOAD_RESPONSE status=200 type=json stage=private failure=body',
                       b'OWNED_UPLOAD_RESPONSE status=200 type=json stage=upload failure=private',
                       b'OWNED_UPLOAD_RESPONSE status=200 type=json stage=upload failure=body secret=value'):
            with self.subTest(marker=marker):
                with self.assertRaisesRegex(AssertionError, r'^owned upload browser rejected at quick-reply-upload\.mjs$'):
                    finish_browser(self.process(marker + b'\n'), 'quick-reply-upload.mjs')

    def test_drawing_lane_uses_same_bounded_private_diagnostics(self):
        error = (b'private drawing receipt ' + b'a' * 64
                 + b'\n at file:///owned/tests/browser/drawing-upload.mjs:72:11\n')
        with self.assertRaisesRegex(AssertionError, r'^owned upload browser rejected at drawing-upload\.mjs:72:11$'):
            finish_browser(self.process(error), 'drawing-upload.mjs')

    def test_unknown_diagnostic_script_is_rejected_before_child_access(self):
        with self.assertRaises(AssertionError):
            finish_browser(None, 'private-script.mjs')


if __name__ == '__main__':
    unittest.main()
