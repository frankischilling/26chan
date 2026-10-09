"""Offline regressions for the isolated format-metadata evidence."""
import hashlib
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location('format_metadata', Path(__file__).with_name('extract-board-format-metadata-reference.py'))
metadata = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(metadata)


class BoardFormatMetadataTests(unittest.TestCase):
    def test_fixture_matches_pinned_capture(self):
        self.assertEqual(metadata.FIXTURE.read_bytes(), metadata.encoded())
        fixture = json.loads(metadata.FIXTURE.read_bytes())
        self.assertEqual(hashlib.sha256(fixture['source_excerpt']['text'].encode()).hexdigest(), metadata.BLOCK_HASH)

    def test_independent_four_case_truth_table(self):
        cases = json.loads(metadata.FIXTURE.read_bytes())['cases']
        self.assertEqual([(row['code_tags'], row['sjis_tags']) for row in cases],
                         [(False, False), (False, True), (True, False), (True, True)])
        self.assertEqual([row['expected'] for row in cases],
                         [{}, {'sjis_tags': 1}, {'code_tags': 1}, {'code_tags': 1, 'sjis_tags': 1}])
        for row in cases:
            for value in row['expected'].values():
                self.assertIs(type(value), int)
                self.assertEqual(value, 1)

    def test_unpinned_source_rejected_without_execution(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'imgboard.php').write_text("<?php throw new Exception('must not run');")
            with self.assertRaisesRegex(ValueError, 'Source hash mismatch'):
                metadata.capture(root)


if __name__ == '__main__':
    unittest.main()
