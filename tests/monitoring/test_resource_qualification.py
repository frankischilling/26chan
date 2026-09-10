import importlib.util
from pathlib import Path
import tempfile
import unittest


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
        labels = {'alertname': 'BoardStorageBytesPressure', 'job': 'board-resource',
                  'instance': '127.0.0.1:9999', 'storage': 'database'}
        firing = {'status': 'firing', 'labels': labels, 'fingerprint': 'owned'}
        self.assertIs(module.find_notification([firing], labels, 'firing'), firing)
        resolved = {**firing, 'status': 'resolved'}
        module.assert_same_alert(firing, resolved)
        with self.assertRaises(AssertionError):
            module.assert_same_alert(firing, {**resolved, 'fingerprint': 'other'})
        with self.assertRaises(AssertionError):
            module.find_notification([{**firing, 'labels': {**labels, 'instance': 'other'}}], labels, 'firing')

    def test_fixture_refuses_unowned_or_noncanonical_roots(self):
        module = self.module('resource_fixture')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            for bad in (root, root / 'missing', Path('relative')):
                with self.assertRaises(ValueError):
                    module.OwnedFixture(bad)


if __name__ == '__main__':
    unittest.main()
