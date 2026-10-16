"""Public CAPTCHA policy extraction regressions; no source PHP is executed."""
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    'captcha_policy', Path(__file__).with_name('extract-captcha-board-policy.py'))
policy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(policy)


class CaptchaPolicyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        (self.root / 'config/boards').mkdir(parents=True)
        (self.root / 'config/categories').mkdir()
        (self.root / 'boardlist.txt').write_text('g\n')
        (self.root / 'config/global_config.ini').write_text(
            'CATEGORY =\nCAPTCHA = yes\nCAPTCHA_TWISTER = yes\n'
            'CAPTCHA_ALLOW_BYPASS = yes\nPREUPLOAD_CAPTCHA = no\n'
            'NOSCRIPT_CAPTCHA_ONLY = no\nPRIVATE_UNRELATED = not-for-output\n')
        (self.root / 'config/categories/ws.config.ini').write_text('CAPTCHA_ALLOW_BYPASS = no\n')
        self.board = self.root / 'config/boards/g.config.ini'
        self.board.write_text('CATEGORY = ws\nCAPTCHA_TWISTER = no\n')

    def test_independent_switches_and_layer_precedence(self):
        result = policy.capture(self.root)
        self.assertEqual(result['boards'], [{
            'slug': 'g', 'enabled': True, 'twister': False,
            'allow_bypass': False, 'preupload': False, 'noscript_only': False,
        }])
        self.assertNotIn('not-for-output', json.dumps(result))
        self.board.write_text('CATEGORY = ws\nCAPTCHA_ALLOW_BYPASS = yes\n')
        self.assertTrue(policy.capture(self.root)['boards'][0]['allow_bypass'])

    def test_commented_switch_is_not_an_override_and_extra_boards_are_kept(self):
        self.board.write_text('CATEGORY = ws\n; CAPTCHA = no\n')
        (self.root / 'config/boards/test.config.ini').write_text('CATEGORY = ws\nCAPTCHA = no\n')
        rows = policy.capture(self.root)['boards']
        self.assertEqual([r['slug'] for r in rows], ['g', 'test'])
        self.assertTrue(rows[0]['enabled'])
        self.assertFalse(rows[1]['enabled'])

    def test_invalid_or_ambiguous_policy_is_rejected(self):
        for text in ['CATEGORY = ../ws\n', 'CATEGORY =\n',
                     'CATEGORY = ws\nCAPTCHA = 1\n',
                     'CATEGORY = ws\nCAPTCHA = yes\nCAPTCHA = no\n']:
            with self.subTest(text=text):
                self.board.write_text(text)
                with self.assertRaises(ValueError):
                    policy.capture(self.root)

    def test_pinned_fixture_preserves_each_exception(self):
        path = Path(__file__).resolve().parents[1] / 'fixtures/captcha-board-policy.json'
        rows = json.loads(path.read_text())['boards']
        self.assertEqual(len(rows), 82)
        for column, expected in [
            ('enabled', {'j'}), ('twister', {'f'}),
            ('allow_bypass', {'pol', 'trash', 'bant'}),
        ]:
            self.assertEqual({row['slug'] for row in rows if not row[column]}, expected)
        self.assertEqual({row['slug'] for row in rows if row['preupload']}, {'hr'})
        self.assertFalse(any(row['noscript_only'] for row in rows))


if __name__ == '__main__':
    unittest.main()
