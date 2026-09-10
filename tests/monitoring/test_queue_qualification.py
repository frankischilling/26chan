import importlib.util
from pathlib import Path
import unittest


class QueueQualificationTests(unittest.TestCase):
    def module(self):
        path = Path(__file__).with_name('queue_qualify.py')
        self.assertTrue(path.is_file(), 'Queue qualification is not implemented')
        spec = importlib.util.spec_from_file_location('queue_qualify', path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_acceleration_preserves_thresholds_and_retention_expressions(self):
        module = self.module()
        source = 'expr: time() - sample <= 30 and rate(errors[15m]) > 0.9\nfor: 30s\nfor: 1m\nfor: 2m\n'
        self.assertEqual(module.accelerated_rules(source),
                         'expr: time() - sample <= 30 and rate(errors[15m]) > 0.9\nfor: 4s\nfor: 4s\nfor: 2m\n')

    def test_unavailable_sample_rejects_retained_queue_values(self):
        module = self.module()
        unavailable = b'board_media_sample_success 0\nboard_media_sample_last_success_timestamp_seconds 123\n'
        module.assert_unavailable(503, unavailable)
        for family in ('board_media_queue_capacity', 'board_media_jobs{state="queued"}',
                       'board_media_expired_jobs{state="queued"}', 'board_media_oldest_queued_seconds',
                       'board_media_failures_recent{reason="processing_failed"}'):
            with self.assertRaises(AssertionError):
                module.assert_unavailable(503, unavailable + (family + ' 0\n').encode())
        with self.assertRaises(AssertionError):
            module.assert_unavailable(200, unavailable)
        with self.assertRaises(AssertionError):
            module.assert_unavailable(503, unavailable.replace(b'success 0', b'success 1'))

    def test_alert_evidence_requires_the_owned_target_and_matching_fingerprint(self):
        module = self.module()
        firing = {'status': 'firing', 'labels': {'alertname': 'BoardMediaQueuePressure',
                  'job': 'board-monitor', 'instance': '127.0.0.1:9194'}, 'fingerprint': 'owned'}
        self.assertIs(module.find_notification([firing], 'BoardMediaQueuePressure', 'firing',
                                              '127.0.0.1:9194'), firing)
        with self.assertRaises(AssertionError):
            module.find_notification([firing], 'BoardMediaQueuePressure', 'firing', '127.0.0.1:9999')
        resolved = {**firing, 'status': 'resolved'}
        module.assert_same_alert(firing, resolved)
        with self.assertRaises(AssertionError):
            module.assert_same_alert(firing, {**resolved, 'fingerprint': 'different'})


if __name__ == '__main__':
    unittest.main()
