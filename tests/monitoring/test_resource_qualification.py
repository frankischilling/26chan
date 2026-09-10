import importlib.util
import base64
from contextlib import redirect_stdout
import http.server
import io
import json
import os
from pathlib import Path
import tempfile
import shutil
import ssl
import threading
import unittest
import urllib.parse


class ResourceQualificationTests(unittest.TestCase):
    def module(self, name):
        path = Path(__file__).with_name(name + '.py')
        self.assertTrue(path.is_file(), 'Resource qualification is not implemented')
        spec = importlib.util.spec_from_file_location(name, path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_acceleration_changes_time_constants_only(self):
        module = self.module('resource_qualify')
        original = 'expr: rate(counter[5m]) > 0.2 and ratio > 0.9 and time() - stamp <= 30\nfor: 2m\nfor: 30s\n'
        self.assertEqual(module.accelerated_rules(original),
                         'expr: rate(counter[30s]) > 0.2 and ratio > 0.9 and time() - stamp <= 30\nfor: 4s\nfor: 4s\n')

    def test_metrics_require_one_finite_owned_target(self):
        module = self.module('resource_qualify')
        sample = b'board_service_tasks{service="public"} 15\n'
        self.assertEqual(module.metric(sample, 'board_service_tasks', 'service', 'public'), 15)
        for bad in (b'', sample + sample, sample.replace(b'15', b'NaN'),
                    sample.replace(b'public', b'staff')):
            with self.assertRaises(AssertionError):
                module.metric(bad, 'board_service_tasks', 'service', 'public')

    def test_unavailable_drops_all_data_families(self):
        module = self.module('resource_qualify')
        sample = b'board_resource_sample_success 0\nboard_resource_sample_last_success_timestamp_seconds 1\n'
        module.assert_unavailable(503, sample)
        for bad in (sample + b'board_storage_read_only{storage="database"} 0\n',
                    sample + b'board_service_tasks{service="public"} 1\n'):
            with self.assertRaises(AssertionError):
                module.assert_unavailable(503, bad)
        with self.assertRaises(AssertionError):
            module.assert_unavailable(200, sample)

    def test_notifications_require_owned_labels_and_same_fingerprint(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'parse_starts_at', None)), 'Activation timestamp parsing is not implemented')
        labels = {'alertname': 'BoardStorageBytesPressure', 'job': 'board-resource',
                  'instance': '127.0.0.1:9999', 'storage': 'database'}
        firing = {'status': 'firing', 'labels': labels, 'fingerprint': 'owned', 'startsAt': '2026-09-10T12:00:00Z'}
        self.assertIs(module.find_notification([firing], labels, 'firing', activation_ns=0), firing)
        resolved = {**firing, 'status': 'resolved'}
        module.assert_same_alert(firing, resolved)
        with self.assertRaises(AssertionError):
            module.assert_same_alert(firing, {**resolved, 'fingerprint': 'other'})
        with self.assertRaises(AssertionError):
            module.find_notification([{**firing, 'labels': {**labels, 'instance': 'other'}}], labels, 'firing', activation_ns=0)

    def test_delayed_old_pair_cannot_qualify_a_new_activation(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'parse_starts_at', None)), 'Activation timestamp parsing is not implemented')
        labels = {'alertname': 'BoardServiceCpuThrottling', 'job': 'board-resource',
                  'instance': '127.0.0.1:9999', 'service': 'public'}
        old = {'status': 'firing', 'labels': labels, 'fingerprint': 'same', 'startsAt': '2026-09-10T12:00:00Z'}
        current = {**old, 'startsAt': '2026-09-10T12:00:02.000000001Z'}
        boundary = module.parse_starts_at('2026-09-10T12:00:02Z')
        old_pair = [old, {**old, 'status': 'resolved'}]
        self.assertIsNone(module.find_notification(old_pair, labels, 'firing', activation_ns=boundary))
        self.assertIs(module.find_notification([*old_pair, current, {**current, 'status': 'resolved'}],
                                              labels, 'firing', activation_ns=boundary), current)
        self.assertIsNone(module.find_notification(old_pair, labels, 'resolved', activation_ns=boundary, firing=current))

    def test_resolution_skips_old_generation_and_selects_exact_current_identity(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'parse_starts_at', None)), 'Activation timestamp parsing is not implemented')
        labels = {'alertname': 'BoardResourceObserverUnavailable', 'job': 'board-resource', 'instance': '127.0.0.1:9999'}
        firing = {'status': 'firing', 'labels': labels, 'fingerprint': 'same', 'startsAt': '2026-09-10T12:00:02Z'}
        resolved = {**firing, 'status': 'resolved'}
        old_resolved = {**resolved, 'startsAt': '2026-09-10T12:00:00Z'}
        wrong_fingerprint = {**resolved, 'fingerprint': 'other'}
        wrong_extra_label = {**resolved, 'labels': {**labels, 'severity': 'other'}}
        evidence = [firing, old_resolved, wrong_fingerprint, wrong_extra_label, resolved]
        selected = module.find_notification(evidence, labels, 'resolved', activation_ns=0, firing=firing)
        self.assertIs(selected, resolved)
        module.assert_same_alert(firing, selected)

    def test_start_time_parser_requires_bounded_aware_rfc3339_and_keeps_nanoseconds(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'parse_starts_at', None)), 'Activation timestamp parsing is not implemented')
        epoch = module.parse_starts_at('1970-01-01T00:00:00Z')
        self.assertEqual(epoch, 0)
        self.assertEqual(module.parse_starts_at('1970-01-01T01:00:00.000000001+01:00'), 1)
        for invalid in (None, '', 1, '2026-09-10T12:00:00', '2026-09-10 12:00:00Z',
                        '2026-02-30T12:00:00Z', '2026-09-10T12:00:60Z',
                        '2026-09-10T12:00:00-00:00', '2026-09-10T12:00:00+24:00',
                        '2026-09-10T12:00:00.1234567890Z', 'x' * 10000):
            with self.assertRaises(ValueError):
                module.parse_starts_at(invalid)

    def test_missing_or_invalid_generation_can_never_match(self):
        module = self.module('resource_qualify')
        for invalid in (None, '', 'invalid', '2026-09-10T12:00:00'):
            firing = {'status': 'firing', 'labels': {'alertname': 'BoardServiceCpuThrottling'},
                      'fingerprint': 'same', 'startsAt': invalid}
            with self.assertRaises(AssertionError):
                module.assert_same_alert(firing, {**firing, 'status': 'resolved'})

    def test_alertmanager_boundary_uses_verified_tls_auth_and_includes_all_current_states(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'alertmanager_clear', None)), 'Alertmanager healthy boundary is not implemented')
        with tempfile.TemporaryDirectory(prefix='resource-api-test-') as temporary:
            openssl = os.environ.get('OPENSSL_BIN') or shutil.which('openssl') or 'C:/Program Files/Git/usr/bin/openssl.exe'
            pki = module.make_pki(Path(temporary) / 'pki', openssl)
            password = 'a' * 64
            expected_auth = 'Basic ' + base64.b64encode(('operator:' + password).encode()).decode()
            observations = []
            response_body = [b'[]']

            class Handler(http.server.BaseHTTPRequestHandler):
                def log_message(self, *_args):
                    pass

                def do_GET(self):
                    authorized = self.headers.get('Authorization') == expected_auth
                    observations.append((self.path, authorized, self.connection.version()))
                    data = response_body[0] if authorized else b''
                    self.send_response(200 if authorized else 401)
                    self.send_header('Content-Length', str(len(data)))
                    self.end_headers()
                    self.wfile.write(data)

            server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
            tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            tls.minimum_version = ssl.TLSVersion.TLSv1_3
            tls.load_cert_chain(pki['cert'], pki['key'])
            server.socket.settimeout(2)
            server.socket = tls.wrap_socket(server.socket, server_side=True)
            worker = threading.Thread(target=lambda: server.serve_forever(poll_interval=0.05), daemon=False)
            worker.start()
            try:
                url = 'https://localhost:' + str(server.server_port)
                name = 'BoardServiceCpuThrottling'
                instance = '127.0.0.1:9999'
                basic = ('operator', password)
                self.assertTrue(module.alertmanager_clear(url, pki['ca'], basic, name, instance))
                path, authorized, protocol = observations[-1]
                self.assertTrue(authorized)
                self.assertEqual(protocol, 'TLSv1.3')
                parsed = urllib.parse.urlsplit(path)
                self.assertEqual(parsed.path, '/api/v2/alerts')
                self.assertEqual(urllib.parse.parse_qs(parsed.query),
                                 {'active': ['true'], 'silenced': ['true'], 'inhibited': ['true'],
                                  'unprocessed': ['true'], 'filter': ['alertname="' + name + '"',
                                  'job="board-resource"', 'instance="' + instance + '"']})
                for bad in (None, ('operator', 'b' * 64)):
                    with self.assertRaises(AssertionError):
                        module.alertmanager_clear(url, pki['ca'], bad, name, instance)
                with self.assertRaises(ssl.SSLCertVerificationError):
                    module.alertmanager_clear(url, pki['wrong_ca'], basic, name, instance)
                self.assertTrue(module.alertmanager_clear(url, pki['ca'], basic, name, instance))
                labels = {'alertname': name, 'job': 'board-resource', 'instance': instance}
                for state in ('active', 'suppressed', 'unprocessed'):
                    response_body[0] = json.dumps([{'labels': labels, 'status': {'state': state}}]).encode()
                    self.assertFalse(module.alertmanager_clear(url, pki['ca'], basic, name, instance))
                for malformed in (b'{}', b'null', b'[null]', b'not-json', b'[{"labels":{}}]',
                                  b'[{"labels":{},"labels":{}}]',
                                  json.dumps([{'labels': {**labels, 'instance': 'other'},
                                               'status': {'state': 'active'}}]).encode()):
                    response_body[0] = malformed
                    with self.assertRaises(AssertionError):
                        module.alertmanager_clear(url, pki['ca'], basic, name, instance)
                response_body[0] = b'[]'
                self.assertTrue(module.alertmanager_clear(url, pki['ca'], basic, name, instance))
            finally:
                server.shutdown()
                server.server_close()
                worker.join(timeout=3)
            self.assertFalse(worker.is_alive())

    def test_fixture_refuses_unowned_or_noncanonical_roots(self):
        module = self.module('resource_fixture')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            for bad in (root, root / 'missing', Path('relative')):
                with self.assertRaises(ValueError):
                    module.OwnedFixture(bad)

    def test_phase_failure_reports_only_the_closed_stage_and_preserves_failure(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'phase', None)), 'Bounded phase diagnostics are not implemented')
        captured = io.StringIO()
        failure = AssertionError('private-input-must-not-be-printed')

        def fail():
            raise failure

        with redirect_stdout(captured), self.assertRaises(AssertionError) as caught:
            module.phase('BoardServiceCpuThrottling', 'native pressure', fail)
        self.assertIs(caught.exception, failure)
        self.assertEqual(captured.getvalue(),
                         'STAGE BoardServiceCpuThrottling native pressure\n'
                         'STAGE failed BoardServiceCpuThrottling native pressure\n')

    def test_phase_labels_reject_unbounded_input_without_logging_it(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'phase', None)), 'Bounded phase diagnostics are not implemented')
        captured = io.StringIO()
        with redirect_stdout(captured):
            for name, label in (('private-input', 'native pressure'), ('BoardServiceCpuThrottling', 'private-input')):
                with self.assertRaises(ValueError):
                    module.phase(name, label, lambda: None)
            self.assertEqual(module.phase('BoardServiceCpuThrottling', 'native recovery', lambda: 7), 7)
        self.assertEqual(captured.getvalue(), 'STAGE BoardServiceCpuThrottling native recovery\n')

    def test_cpu_snapshot_contains_only_allowlisted_finite_scalar_metrics(self):
        module = self.module('resource_qualify')
        self.assertTrue(callable(getattr(module, 'cpu_snapshot', None)), 'Safe CPU diagnostics are not implemented')
        source = (b'board_resource_sample_success 1\n'
                  b'board_service_cpu_usage_seconds_total{service="public"} 12.5\n'
                  b'board_service_cpu_quota_cores{service="public"} 0.2\n'
                  b'board_service_cpu_periods_total{service="public"} 44\n'
                  b'board_service_cpu_throttled_periods_total{service="public"} NaN\n'
                  b'private_input{secret="must-not-escape"} 123\n')
        snapshot = module.cpu_snapshot(source)
        self.assertEqual(snapshot, {'board_resource_sample_success': 1,
                                   'board_service_cpu_usage_seconds_total': 12.5,
                                   'board_service_cpu_quota_cores': 0.2,
                                   'board_service_cpu_periods_total': 44,
                                   'board_service_cpu_throttled_periods_total': None})


if __name__ == '__main__':
    unittest.main()
