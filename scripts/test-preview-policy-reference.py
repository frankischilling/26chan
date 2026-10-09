#!/usr/bin/env python3
"""Pure offline preview-policy fixture and public-board import regressions."""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


ref = module('preview_policy_reference', 'extract-preview-policy-reference.py')
boards = module('board_reference', 'extract-board-reference.py')


class PreviewPolicyReferenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = ref.load_fixture()
        cls.cases = {case['id']: case for case in cls.fixture['cases']}

    def case(self, limit, sticky, replies, pattern='text'):
        return self.cases[f'limit-{limit}-sticky-{int(sticky)}-replies-{replies}-{pattern}']

    def test_pinned_source_and_truth_table_scope(self):
        self.assertEqual(self.fixture['source_revision'], '545b7812d1849f7958d914950c91fdbbe38f6b22')
        self.assertIn('No original PHP', self.fixture['source_execution'])
        self.assertEqual(len(self.cases), 172)
        self.assertEqual({case['configured_limit'] for case in self.cases.values()}, {0, 1, 3, 5})
        for limit in (0, 1, 3, 5):
            for sticky in (False, True):
                for count in range(7):
                    for pattern in ('text', 'images', 'mixed-deletions'):
                        self.assertIn(self.case(limit, sticky, count, pattern), self.fixture['cases'])

    def test_sticky_caps_and_zero_are_not_truthy_defaults(self):
        for sticky in (False, True):
            self.assertEqual(self.case(0, sticky, 6)['expected']['visible_reply_ids'], [])
        for limit in (1, 3, 5):
            expected = self.case(limit, True, 6)['expected']
            self.assertEqual(expected['effective_limit'], 1)
            self.assertEqual(expected['visible_reply_ids'], ['106'])
            self.assertEqual(expected['omitted_posts_count'], 5)
        self.assertEqual(self.case(3, False, 6)['expected']['visible_reply_ids'], ['104', '105', '106'])
        self.assertEqual(self.case(5, False, 6)['expected']['visible_reply_ids'], ['102', '103', '104', '105', '106'])

    def test_zero_reply_catalog_key_absent_but_zero_limit_is_empty(self):
        for limit in (0, 1, 3, 5):
            for sticky in (False, True):
                empty = self.case(limit, sticky, 0)['expected']
                self.assertNotIn('last_reply_ids', empty['catalog'])
                self.assertEqual(empty['index']['post_ids'], ['100'])
                self.assertEqual(empty['index']['op_extra'], {'replies': 0, 'images': 0})
        for sticky in (False, True):
            zero = self.case(0, sticky, 3)['expected']
            self.assertIn('last_reply_ids', zero['catalog'])
            self.assertEqual(zero['catalog']['last_reply_ids'], [])
            self.assertEqual(zero['index']['post_ids'], ['100'])

    def test_omission_fields_present_together_only_if_posts_omitted(self):
        for case in self.cases.values():
            expected = case['expected']
            for extra in (expected['index']['op_extra'], expected['catalog']['op_extra']):
                if expected['omitted_posts_count']:
                    self.assertEqual(extra['omitted_posts'], expected['omitted_posts_count'])
                    self.assertEqual(extra['omitted_images'], expected['omitted_images_count'])
                else:
                    self.assertNotIn('omitted_posts', extra)
                    self.assertNotIn('omitted_images', extra)
            full = expected['full_thread']['op_extra']
            self.assertNotIn('omitted_posts', full)
            self.assertNotIn('omitted_images', full)
        self.assertEqual(self.case(3, False, 5)['expected']['index']['op_extra'],
                         {'replies': 5, 'images': 0, 'omitted_posts': 2, 'omitted_images': 0})

    def test_image_counts_exclude_op_and_deleted_files(self):
        # OP has a file in every fixture, including these zero-image results.
        text = self.case(3, False, 6)['expected']
        self.assertEqual((text['image_count'], text['omitted_images_count']), (0, 0))
        all_images = self.case(3, False, 6, 'images')['expected']
        self.assertEqual((all_images['image_count'], all_images['visible_image_count'], all_images['omitted_images_count']), (6, 3, 3))
        mixed = self.case(3, False, 6, 'mixed-deletions')['expected']
        self.assertEqual(mixed['reply_count'], 6)
        self.assertEqual((mixed['image_count'], mixed['visible_image_count'], mixed['omitted_images_count']), (2, 1, 1))
        zero = self.case(0, True, 6, 'mixed-deletions')['expected']
        self.assertEqual(zero['index']['op_extra'], {'replies': 6, 'images': 2, 'omitted_posts': 6, 'omitted_images': 2})

    def test_deleted_posts_are_explicitly_prefiltered_controlled_inputs(self):
        case = self.case(5, False, 6, 'mixed-deletions')
        excluded = {row['id'] for row in case['input']['excluded_deleted_replies']}
        self.assertEqual(excluded, {'107', '200'})
        self.assertTrue(excluded.isdisjoint(case['expected']['full_thread']['post_ids']))
        self.assertEqual(case['expected']['reply_count'], 6)
        self.assertTrue(any('not qualified' in text and 'deletion/RLS' in text for text in self.fixture['limits']))
        # Alter excluded rows; the source rendering oracle sees only live cache rows.
        data = copy.deepcopy(case['input'])
        data['excluded_deleted_replies'] *= 5
        self.assertEqual(ref.derive(5, False, data), case['expected'])

    def test_numeric_order_and_last_n_selection(self):
        data = dict(op_id='1', op_fsize=0, replies=[
            dict(id=str(id), fsize=0, file_deleted=False) for id in (11, 9, 10)],
            excluded_deleted_replies=[], show_thread_uniques=False, unique_ips=0)
        expected = ref.derive(1, False, data)
        self.assertEqual(expected['full_thread']['post_ids'], ['1', '9', '10', '11'])
        self.assertEqual(expected['index']['post_ids'], ['1', '11'])
        self.assertEqual(expected['catalog']['last_reply_ids'], ['11'])

    def test_unique_ips_full_thread_only_and_positive(self):
        for enabled in (False, True):
            for count in (0, 4):
                case = self.cases[f'uniques-enabled-{int(enabled)}-count-{count}']['expected']
                self.assertNotIn('unique_ips', case['index']['op_extra'])
                self.assertNotIn('unique_ips', case['catalog']['op_extra'])
                if enabled and count:
                    self.assertEqual(case['full_thread']['op_extra']['unique_ips'], count)
                else:
                    self.assertNotIn('unique_ips', case['full_thread']['op_extra'])

    def test_exact_config_import_and_historical_migration_exclusion(self):
        imported = json.loads(ref.BOARD_FIXTURE.read_text())
        expected = {'b': 3, 'bant': 3, 't': 1, 'vg': 0}
        self.assertEqual(len(imported['boards']), 82)
        self.assertIn('REPLIES_SHOWN', boards.POLICY_KEYS)
        for board in imported['boards']:
            self.assertEqual(board['replies_shown'], expected.get(board['slug'], 5))
            self.assertEqual(board['source_policy']['REPLIES_SHOWN'], str(expected.get(board['slug'], 5)))
        self.assertNotIn('replies_shown', boards.migration(imported))

    def test_each_excerpt_and_line_ending_drift_fail(self):
        for name in ref.EXCERPTS:
            captured = copy.deepcopy(self.fixture['captured_sources'])
            captured[name] += '\n'
            with self.assertRaisesRegex(ValueError, 'captured source drift'):
                ref.generate(captured)
        captured = {name: text.replace('\r\n', '\n') for name, text in self.fixture['captured_sources'].items()}
        with self.assertRaisesRegex(ValueError, 'captured source drift'):
            ref.generate(captured)

    def test_exact_line_extraction_rejects_changed_source(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            for file in {spec[0] for spec in ref.EXCERPTS.values()}:
                lines = [b'\r\n'] * max(spec[2] for spec in ref.EXCERPTS.values() if spec[0] == file)
                for name, (path, start, end, _) in ref.EXCERPTS.items():
                    if path == file:
                        lines[start-1:end] = self.fixture['captured_sources'][name].encode().splitlines(keepends=True)
                target = source / file
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(b''.join(lines))
            self.assertEqual(ref.capture(source), self.fixture['captured_sources'])
            target = source / 'config/global_config.ini'
            target.write_bytes(target.read_bytes().replace(b'REPLIES_SHOWN = 5', b'REPLIES_SHOWN = 3'))
            with self.assertRaisesRegex(ValueError, 'source drift: global_limit'):
                ref.capture(source)

    def test_fixture_expectation_tampering_fails(self):
        original = ref.FIXTURE
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'preview.json'
            changed = copy.deepcopy(self.fixture)
            changed['cases'][0]['expected']['catalog']['last_reply_ids'] = []
            target.write_text(json.dumps(changed))
            try:
                ref.FIXTURE = target
                with self.assertRaisesRegex(ValueError, 'fixture differs'):
                    ref.load_fixture()
            finally:
                ref.FIXTURE = original


if __name__ == '__main__':
    unittest.main()
