"""Offline tests for pinned source extraction and controlled presentation cases.

These tests do not execute PHP, Rust, SQL, Git, browsers, or network operations.
"""
import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location('quote_reference', Path(__file__).with_name('extract-quote-resolution-reference.py'))
ref = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ref)


class QuoteResolutionReferenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = ref.load_fixture()
        cls.cases = {case['id']: case for case in cls.fixture['cases']}

    def test_pinned_revision_and_method_are_explicit(self):
        self.assertEqual(self.fixture['source_revision'], '545b7812d1849f7958d914950c91fdbbe38f6b22')
        self.assertEqual(self.fixture['oracle_method'], 'static-extraction-and-independently-derived-controlled-lookup-expectations')
        self.assertIn('not performed', self.fixture['source_execution'])
        self.assertEqual(len(self.cases), len(self.fixture['cases']))

    def test_full_exact_allowlist_is_source_derived(self):
        expected = '3|aco|adv|an|biz|diy|fa|fit|gd|gif|int|lit|hc|hr|a|b|ck|co|cm|c|d|e|f|g|h|i|k|lgbt|m|n|o|out|p|r|s|t|u|vp|vg|vr|v|w|x|y|wg|ic|cgl|hm|mlp|mu|pol|po|r9k|s4s|sci|soc|tg|tv|toy|trv|jp|sp|wsg|qa|qst|his|trash|news|wsr|vip|bant|vrpg|vmg|vst|vt|vm|pw|xs'.split('|')
        self.assertEqual(self.fixture['allowlist'], expected)
        self.assertEqual(len(set(expected)), len(expected))
        for board in expected:
            self.assertEqual(self.cases['allowlisted-' + board]['expected']['href'], f'//boards.example.test/{board}/thread/123#p123')

    def test_local_other_thread_and_index_destinations(self):
        expected = {'same-thread-op': '#p100', 'same-thread-reply': '#p123',
                    'other-thread-op': '/g/thread/123#p123', 'other-thread-reply': '/g/thread/200#p123',
                    'board-index-op': '/g/thread/123#p123', 'board-index-reply': '/g/thread/100#p123'}
        for id, href in expected.items():
            self.assertEqual(self.cases[id]['expected']['href'], href)
        self.assertEqual(self.cases['same-thread-op']['expected']['thread_id'], '100')

    def test_cross_board_replies_use_thread_not_post_id(self):
        self.assertEqual(self.cases['cross-op']['expected']['href'], '//boards.example.test/a/thread/123#p123')
        self.assertEqual(self.cases['cross-reply']['expected']['href'], '//boards.example.test/a/thread/200#p123')
        self.assertEqual(self.cases['cross-same-board-remains-full-url']['expected']['href'], '//boards.example.test/g/thread/100#p123')

    def test_controlled_absence_is_dead_and_not_a_database_claim(self):
        for absence in ('missing', 'private-as-missing', 'deleted'):
            for prefix in ('same-', 'cross-'):
                case = self.cases[prefix + absence]
                self.assertIsNone(case['lookup_resto'])
                self.assertEqual(case['absence_reason'], absence)
                self.assertEqual(case['expected']['kind'], 'dead')
                self.assertEqual(case['expected']['html'], '<span class="deadlink">' + case['label_html'] + '</span>')
                self.assertIsNone(case['expected']['href'])
        self.assertTrue(any('not evidence of source SQL RLS' in text for text in self.fixture['limits']))

    def test_allowlist_rejection_is_plain_even_with_positive_lookup(self):
        for board in ('unknown', 'test', 'global'):
            case = self.cases['cross-allowlist-reject-' + board]
            self.assertEqual(case['lookup_resto'], '100')
            self.assertEqual(case['expected']['kind'], 'plain')
            self.assertEqual(case['expected']['html'], case['label_html'])

    def test_mlp_restriction_ignores_op_reply_and_absence(self):
        for board in ('b', 'co'):
            for suffix in ('0', '200', 'missing'):
                self.assertEqual(self.cases[f'mlp-{board}-{suffix}']['expected']['kind'], 'dead')
        self.assertEqual(self.cases['mlp-allowed-a']['expected']['kind'], 'thread')
        self.assertEqual(self.cases['other-source-can-link-b']['expected']['kind'], 'thread')

    def test_canonical_boundary_and_unqualified_mysql_coercion(self):
        self.assertTrue(ref.canonical('1'))
        self.assertTrue(ref.canonical('9223372036854775807'))
        for value in self.fixture['unqualified_ids'] + ['', ' 1', '١', '１２', 1, None]:
            self.assertFalse(ref.canonical(value), repr(value))
        for case in self.cases.values():
            self.assertTrue(ref.canonical(case['target_post_id']))
        self.assertEqual(self.cases['maximum-canonical-reply']['expected']['href'], '//boards.example.test/a/thread/9223372036854775806#p9223372036854775807')

    def test_escaped_lexical_labels_and_token_prefixes(self):
        cases = {case['id']: case for case in self.fixture['lexical_cases']}
        for id in ('double-escaped-not-reinterpreted', 'uppercase-cross-board-is-plain'):
            self.assertEqual(cases[id]['input_html'], cases[id]['expected_html'])
        self.assertEqual(cases['same-digit-prefix-before-letters']['expected_html'], '<a href="#p123" class="quotelink">&gt;&gt;123</a>abc')
        self.assertEqual(cases['same-digit-prefix-before-exponent']['expected_html'], '<a href="#p1" class="quotelink">&gt;&gt;1</a>e3')
        escaped = cases['escaped-prose-preserved']['expected_html']
        self.assertTrue(escaped.startswith('&lt;b&gt; &amp; &quot; '))
        self.assertTrue(escaped.endswith(' &lt;/b&gt;'))
        for case in self.cases.values():
            self.assertIn(case['label_html'], case['expected']['html'])
            self.assertNotIn('&amp;gt;', case['expected']['html'])

    def test_source_lookup_and_json_evidence_is_captured(self):
        sources = self.fixture['captured_sources']
        self.assertIn("return $log[$no]['resto'];", sources['same_lookup'])
        self.assertIn("$board != BOARD_DIR && !isset( $boardlist[$board] )", sources['cross_lookup'])
        self.assertIn("$var['com'] = auto_link( $var['com'], $threadid )", sources['json_auto_link'])

    def test_each_excerpt_tamper_and_line_ending_drift_fails(self):
        for name in ref.EXCERPTS:
            mutated = copy.deepcopy(self.fixture['captured_sources'])
            mutated[name] += '\n'
            with self.assertRaisesRegex(ValueError, 'captured source drift'):
                ref.generate(mutated)
        mutated = {name: text.replace('\r\n', '\n') for name, text in self.fixture['captured_sources'].items()}
        with self.assertRaisesRegex(ValueError, 'captured source drift'):
            ref.generate(mutated)

    def test_source_line_range_extraction_and_drift_detection(self):
        # Reconstruct only the pinned line ranges in a temporary synthetic tree.
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            for file in {spec[0] for spec in ref.EXCERPTS.values()}:
                max_line = max(spec[2] for spec in ref.EXCERPTS.values() if spec[0] == file)
                lines = [b'\r\n'] * max_line
                for name, (path, start, end, _) in ref.EXCERPTS.items():
                    if path == file:
                        lines[start-1:end] = self.fixture['captured_sources'][name].encode().splitlines(keepends=True)
                (source / file).write_bytes(b''.join(lines))
            self.assertEqual(ref.capture(source), self.fixture['captured_sources'])
            file = source / 'imgboard.php'
            file.write_bytes(file.read_bytes().replace(b'$valid_boards', b'$wrong_boards', 1))
            with self.assertRaisesRegex(ValueError, 'source drift: allowlist'):
                ref.capture(source)

    def test_expected_fixture_tampering_is_rejected(self):
        original = ref.FIXTURE
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'reference.json'
            changed = copy.deepcopy(self.fixture)
            changed['cases'][0]['expected']['href'] = '/incorrect/thread/100'
            path.write_text(json.dumps(changed))
            try:
                ref.FIXTURE = path
                with self.assertRaisesRegex(ValueError, 'fixture differs'):
                    ref.load_fixture()
            finally:
                ref.FIXTURE = original


if __name__ == '__main__':
    unittest.main()
