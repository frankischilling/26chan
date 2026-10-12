#!/usr/bin/env python3
"""Check independent archive enablement and age in the captured board policy."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("board_reference", ROOT / "scripts/extract-board-reference.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ArchiveAgeTests(unittest.TestCase):
    def test_disabled_finite_and_unlimited_are_distinct(self):
        for enabled, hours, expected in [(False, 0, 0), (False, 72, 0),
                                         (True, 0, -1), (True, 1, 3600), (True, 72, 259200)]:
            with self.subTest(enabled=enabled, hours=hours):
                self.assertEqual(MODULE.archive_retention(enabled, hours), expected)
        with self.assertRaises(ValueError):
            MODULE.archive_retention(True, -1)

    def test_captured_board_policies_keep_their_existing_values(self):
        reference = json.loads((ROOT / "fixtures/board-reference.json").read_text())
        for board in reference["boards"]:
            policy = board["source_policy"]
            expected = MODULE.archive_retention(policy["ENABLE_ARCHIVE"].lower() == "yes",
                                                int(policy["ARCHIVE_MAX_AGE"]))
            self.assertEqual(board["archive_retention_seconds"], expected, board["slug"])


if __name__ == "__main__":
    unittest.main()
