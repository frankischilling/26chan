#!/usr/bin/env python3
"""Check the bounded subtitle policy without executing source PHP or HTML."""
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("board_reference", ROOT / "scripts/extract-board-reference.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SubtitleImportTests(unittest.TestCase):
    def test_only_audited_source_values_are_accepted(self):
        self.assertEqual(MODULE.subtitle_profile({}), "none")
        self.assertEqual(MODULE.subtitle_profile({"SUBTITLE": MODULE.FICTION_SUBTITLE}), "fiction")
        self.assertEqual(MODULE.subtitle_profile({"SUBTITLE": MODULE.WORKSAFE_SUBTITLE}), "worksafe_gif")
        for value in ["", "fiction", "<script>alert(1)</script>",
                      MODULE.WORKSAFE_SUBTITLE.replace("//boards.4chan.org/wsg/", "javascript:alert(1)"),
                      MODULE.FICTION_SUBTITLE.replace("<br>", "<br onclick=bad()>"),
                      MODULE.FICTION_SUBTITLE + " "]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.subtitle_profile({"SUBTITLE": value})

    def test_captured_policy_regenerates_only_the_additive_migration(self):
        reference = json.loads((ROOT / "fixtures/board-reference.json").read_text())
        self.assertEqual(len(reference["boards"]), 82)
        self.assertEqual({row["slug"]: row["board_subtitle"] for row in reference["boards"]
                          if row["board_subtitle"] != "none"},
                         {"b": "fiction", "trash": "fiction", "gif": "worksafe_gif"})
        for board in reference["boards"]:
            self.assertEqual(MODULE.subtitle_profile(board["source_policy"]), board["board_subtitle"])
        self.assertEqual(MODULE.subtitle_migration(reference),
                         (ROOT / "migrations/0119_board_subtitles.sql").read_text())
        self.assertNotIn("board_subtitle", MODULE.migration(reference))


if __name__ == "__main__":
    unittest.main()
