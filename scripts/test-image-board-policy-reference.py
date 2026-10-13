#!/usr/bin/env python3
"""Check board admission evidence and INI precedence without running source PHP."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("board_reference", ROOT / "scripts/extract-board-reference.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
REFERENCE = json.loads((ROOT / "fixtures/board-reference.json").read_text(encoding="utf-8"))


class ImageBoardPolicyReferenceTests(unittest.TestCase):
    def test_pdf_admission_is_opt_in_and_retains_source_file_limits(self):
        enabled = {row["slug"]: row["max_image_bytes"]
                   for row in REFERENCE["boards"] if row["pdf_enabled"]}
        self.assertEqual(enabled, {"po": 8388608, "sci": 4194304,
                                   "qst": 8388608, "tg": 8388608, "gd": 8388608})
        self.assertNotIn("pdf_enabled", MODULE.migration(REFERENCE),
                         "the already-applied board import must stay byte-stable")

    def test_pinned_board_limits_and_format_restrictions(self):
        boards = {board["slug"]: board for board in REFERENCE["boards"]}
        self.assertEqual(len(boards), 82)
        expected = {
            "a": (4096, 1, 1, 10000, False),
            "b": (2048, 1, 1, 10000, False),
            "bant": (2048, 1, 1, 10000, False),
            "gif": (4096, 1, 1, 10000, True),
            "wsg": (6144, 1, 1, 10000, True),
            "hc": (8192, 500, 500, 8000, False),
            "hr": (8192, 1000, 1000, 10000, False),
            "w": (6144, 480, 600, 10000, False),
            "wg": (6144, 480, 600, 10000, False),
            "f": (10240, 1, 1, 10000, False),
        }
        for slug, (kilobytes, width, height, dimension, gif_only) in expected.items():
            with self.subTest(board=slug):
                row = boards[slug]
                self.assertEqual((row["max_image_bytes"], row["min_image_width"], row["min_image_height"],
                                  row["max_image_dimension"], row["gif_only"]),
                                 (kilobytes * 1024, width, height, dimension, gif_only))
        self.assertEqual({slug for slug, board in boards.items() if board["gif_only"]}, {"gif", "wsg"})
        self.assertTrue((ROOT / "migrations/0132_source_image_board_policy.sql").read_text().startswith(
            MODULE.image_policy_migration(REFERENCE)))
        self.assertNotIn("max_image_bytes", MODULE.migration(REFERENCE))

    def test_category_and_board_override_global_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for folder in ("config/boards", "config/categories", "www.4chan/data"):
                (root / folder).mkdir(parents=True)
            (root / "boardlist.txt").write_text("x y\n")
            (root / "www.4chan/data/boards.php").write_text('"dir"=>"x","name"=>"X"\n"dir"=>"y","name"=>"Y"')
            defaults = REFERENCE["boards"][0]["source_policy"] | {
                "MAX_KB": "4096", "MIN_W": "1", "MIN_H": "1", "MAX_DIMENSION": "10000", "GIF_ONLY": "no", "ENABLE_PDF": "no"}
            (root / "config/global_config.ini").write_text("\n".join(f"{key} = {value}" for key, value in defaults.items()))
            (root / "config/categories/ws.config.ini").write_text("MAX_KB = 2048\nMIN_W = 20\nMAX_DIMENSION = 777\nENABLE_PDF = yes\n")
            (root / "config/boards/x.config.ini").write_text("CATEGORY = ws\nMAX_KB = 128\nMIN_H = 30\nGIF_ONLY = yes\nENABLE_PDF = no\n")
            (root / "config/boards/y.config.ini").write_text("CATEGORY = ws\n")
            rows = MODULE.extract(root)["boards"]
            self.assertEqual([row["pdf_enabled"] for row in rows], [False, True])
            self.assertEqual([(row["max_image_bytes"], row["min_image_width"], row["min_image_height"],
                               row["max_image_dimension"], row["gif_only"]) for row in rows],
                             [(131072, 20, 30, 777, True), (2097152, 20, 1, 777, False)])


if __name__ == "__main__":
    unittest.main()
