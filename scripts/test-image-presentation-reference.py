#!/usr/bin/env python3
"""Verify the reviewed image geometry fixture independently of image codecs."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location(
    'image_reference', ROOT / 'scripts/extract-image-presentation-reference.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
REFERENCE = json.loads((ROOT / 'fixtures/source-image-presentation.json').read_text())


class ImagePresentationReferenceTests(unittest.TestCase):
    def test_pinned_blocks_and_manual_expectations(self):
        self.assertEqual(REFERENCE['source_sha256'], MODULE.SOURCE_SHA)
        self.assertEqual(REFERENCE['cases'], MODULE.cases())
        self.assertEqual(set(REFERENCE['blocks']), set(MODULE.BLOCKS))
        for name, (_, _, digest) in MODULE.BLOCKS.items():
            self.assertEqual(hashlib.sha256(REFERENCE['blocks'][name].encode()).hexdigest(), digest)

    def test_changed_source_is_not_recaptured(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            (root / 'imgboard.php').write_text('<?php // changed source')
            with self.assertRaisesRegex(ValueError, 'reviewed snapshot'):
                MODULE.extract(root)

    def test_zero_thumbnail_is_not_silently_clamped(self):
        cases = {case['id']: case for case in REFERENCE['cases']['thumbnail']}
        self.assertEqual(cases['skinny-zero']['expected'], [0, 250])
        self.assertEqual(cases['wide-zero']['expected'], [125, 0])

    def test_mobile_size_thresholds_and_rounding(self):
        cases = {case['id']: case for case in REFERENCE['cases']['mobile']}
        for name in ['jpeg-at-side', 'jpeg-at-area', 'png-at-bytes', 'thin-no-area']:
            self.assertIsNone(cases[name]['expected'])
        self.assertEqual(cases['jpeg-over-area']['expected'], [1024, 129])
        self.assertEqual(cases['portrait-round-half']['expected'], [129, 1024])
        self.assertEqual(cases['png-over-bytes']['expected'], [1024, 512])


if __name__ == '__main__':
    unittest.main()
