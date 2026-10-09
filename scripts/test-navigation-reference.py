"""Offline source navigation extraction and generated-table regression tests."""
import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location('navigation_reference', Path(__file__).with_name('extract-navigation-reference.py'))
nav = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(nav)


class NavigationReferenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = nav.load_fixture()

    def test_pinned_source_and_generated_table(self):
        self.assertEqual(nav.RUST.read_bytes(), nav.rust_table(self.fixture).encode())
        self.assertEqual(self.fixture['source_revision'], nav.REVISION)
        self.assertEqual(self.fixture['files'], nav.SOURCE_HASHES)

    def test_groups_and_work_safe_equivalence(self):
        f = self.fixture
        self.assertEqual([len(g) for g in f['header_groups']], [27, 2, 4, 4, 40])
        self.assertEqual(nav.header_groups(f['captured_sources']['header.txt']),
                         nav.header_groups(f['captured_sources']['header-ws.txt']))

    def test_labels_decode_once(self):
        labels = dict(row for group in self.fixture['header_groups'] for row in group)
        self.assertEqual({s: labels[s] for s in ['p', 'diy', 'lgbt', 's4s', 'qa', 'vp']}, {
            'p': 'Photo', 'diy': 'Do It Yourself', 'lgbt': 'LGBT',
            's4s': 'Shit 4chan Says', 'qa': 'Question & Answer', 'vp': 'Pokémon'})
        text = self.fixture['captured_sources']['header.txt'].replace('Pok&eacute;mon', '&amp;eacute;')
        self.assertEqual(dict(sum(nav.header_groups(text), []))['vp'], '&eacute;')

    def test_directory_order_and_differences(self):
        directory = self.fixture['directory_labels']
        self.assertEqual(len(directory), 78)
        self.assertEqual(directory[0], ['3', '3DCG'])
        self.assertEqual(directory[-1], ['f', 'Flash'])
        self.assertEqual(self.fixture['directory_only'], ['asp', 'trash'])
        self.assertEqual(self.fixture['header_only'], ['qa'])
        nav.validate_rows(self.fixture['header_groups'], directory)

    def test_duplicate_slug_rejected(self):
        groups = copy.deepcopy(self.fixture['header_groups'])
        groups[0][1] = groups[0][0]
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            nav.validate_rows(groups, self.fixture['directory_labels'])
        directory = copy.deepcopy(self.fixture['directory_labels'])
        directory[1] = directory[0]
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            nav.validate_rows(self.fixture['header_groups'], directory)

    def test_internal_board_rejected(self):
        for slug in ['test', 'qb', 'j']:
            directory = copy.deepcopy(self.fixture['directory_labels'])
            directory[0][0] = slug
            with self.assertRaisesRegex(ValueError, 'internal board'):
                nav.validate_rows(self.fixture['header_groups'], directory)

    def test_group_boundary_and_href_rejected(self):
        text = self.fixture['captured_sources']['header.txt']
        with self.assertRaisesRegex(ValueError, 'group sizes'):
            nav.header_groups(text.replace('[<a href="/i/"', '<a href="/i/"', 1))
        with self.assertRaisesRegex(ValueError, 'href mismatch'):
            nav.header_groups(text.replace('href="/a/"', 'href="/wrong/"', 1))

    def test_label_difference_rejected(self):
        groups = copy.deepcopy(self.fixture['header_groups'])
        groups[0][13][1] = 'Photography'
        with self.assertRaisesRegex(ValueError, 'label differences'):
            nav.validate_rows(groups, self.fixture['directory_labels'])

    def test_source_bytes_are_hash_pinned(self):
        for path in nav.CAPTURED:
            captured = dict(self.fixture['captured_sources'])
            captured[path] += '\n'
            with self.assertRaisesRegex(ValueError, 'source hash mismatch'):
                nav.fixture_from(captured, self.fixture['source_excerpts'])

    def test_excerpt_bytes_and_metadata_are_hash_pinned(self):
        for name in nav.EXCERPTS:
            excerpts = copy.deepcopy(self.fixture['source_excerpts'])
            excerpts[name]['text'] += '\n'
            with self.assertRaisesRegex(ValueError, 'excerpt hash mismatch'):
                nav.fixture_from(self.fixture['captured_sources'], excerpts)
            excerpts = copy.deepcopy(self.fixture['source_excerpts'])
            excerpts[name]['first_line'] += 1
            with self.assertRaisesRegex(ValueError, 'excerpt metadata mismatch'):
                nav.fixture_from(self.fixture['captured_sources'], excerpts)

    def test_fixture_changes_rejected(self):
        for field, value in [('source_revision', 'unverified'), ('files', {}),
                             ('header_groups', []), ('directory_labels', []),
                             ('header_only', []), ('directory_only', []),
                             ('configured_header', 'header-ws.txt'), ('header_parent_nws', {})]:
            fixture = copy.deepcopy(self.fixture)
            fixture[field] = value
            with tempfile.TemporaryDirectory() as temp:
                path = Path(temp) / 'fixture.json'
                path.write_text(json.dumps(fixture))
                with self.assertRaisesRegex(ValueError, 'fixture contract differs'):
                    nav.load_fixture(path)

    def test_source_checkout_drift_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'config').mkdir()
            (root / 'config/global_config.ini').write_text('changed')
            with self.assertRaisesRegex(ValueError, 'source hash mismatch: config/global_config.ini'):
                nav.extract(root)

    def test_configured_header_and_alternative_parent_classes(self):
        self.assertEqual(self.fixture['configured_header'], 'header.txt')
        active = self.fixture['header_parent_nws']['header.txt']
        alternate = self.fixture['header_parent_nws']['header-ws.txt']
        self.assertEqual(len(active), 77)
        self.assertFalse(any(nws for _, nws in active))
        self.assertEqual([s for s, nws in alternate if nws],
                         'b d e f gif h hr r s t u wg i ic r9k s4s hm y aco bant hc pol soc'.split())
        for path, rows in self.fixture['header_parent_nws'].items():
            self.assertEqual(nav.header_classes(self.fixture['captured_sources'][path]), rows)

    def test_classification_uses_immediate_parent(self):
        text = '<span class="boardList"><span class="nwsb"><span><a href="/a/">a</a></span><a href="/b/">b</a></span></span>'
        self.assertEqual(nav.header_classes(text), [['a', False], ['b', True]])

    def test_configured_header_change_rejected(self):
        excerpts = copy.deepcopy(self.fixture['source_excerpts'])
        excerpts['global_header']['text'] = 'NAV_TXT = /www/global/yotsuba/header-ws.txt\n'
        with self.assertRaisesRegex(ValueError, 'configured header changed'):
            nav.configured_header(excerpts)

    def test_mobile_destination_source_excludes_archive_mode(self):
        text = self.fixture['source_excerpts']['mobile_destination']['text']
        self.assertIn("board !== 'f'", text)
        self.assertIn("? 'catalog' : ''", text)
        self.assertNotIn('archive', text)

    def test_safe_deterministic_rust_strings(self):
        self.assertEqual(nav.rust_string('"\\\n\r\t\0\x7fPokémon'),
                         '"\\"\\\\\\u{a}\\u{d}\\u{9}\\u{0}\\u{7f}Pokémon"')
        self.assertEqual(nav.rust_table(self.fixture), nav.rust_table(copy.deepcopy(self.fixture)))
        self.assertIn('const HEADER_NWS_SLUGS: &[&str] = &[];', nav.rust_table(self.fixture))
        self.assertNotIn('domain', nav.rust_table(self.fixture))


if __name__ == '__main__':
    unittest.main()
