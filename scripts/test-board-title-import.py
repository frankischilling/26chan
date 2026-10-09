#!/usr/bin/env python3
"""Check short-title import, immutable migrations and the guarded SQL update."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts/extract-board-reference.py"
SPEC = importlib.util.spec_from_file_location("board_reference", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
REFERENCE = json.loads((ROOT / "fixtures/board-reference.json").read_text(encoding="utf-8"))
SHORT_TITLE = "Sh*t 4chan Says"
OLD_TITLE = "[s4s] - " + SHORT_TITLE
TITLE_MIGRATION = ROOT / "migrations/0120_board_short_titles.sql"


class BoardTitleImportTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.source = Path(self.temporary.name) / "source"
        for directory in ["config/boards", "config/categories", "www.4chan/data"]:
            (self.source / directory).mkdir(parents=True)
        self.write("boardlist.txt", "a vp s4s\n")
        self.write("www.4chan/data/boards.php",
                   '"dir"=>"a","name"=>"Anime &amp; Manga"\n'
                   '"dir"=>"vp","name"=>"Pokémon"\n'
                   '"dir"=>"s4s","name"=>"Different directory name"\n')
        defaults = REFERENCE["boards"][0]["source_policy"] | {"TITLE": "Synthetic default"}
        self.write("config/global_config.ini", "\n".join(f"{key} = {value}" for key, value in defaults.items()))
        self.write("config/categories/ws.config.ini", "\n")
        self.write("config/boards/a.config.ini", "CATEGORY = ws\n")
        self.write("config/boards/vp.config.ini", "CATEGORY = ws\n;TITLE = /vp/ - Ignored comment\n")
        self.write("config/boards/s4s.config.ini", "CATEGORY = ws\nTITLE = " + OLD_TITLE + "\n")

    def write(self, relative, text):
        (self.source / relative).write_text(text, encoding="utf-8")

    def titles(self, **options):
        return {board["slug"]: board["title"] for board in MODULE.extract(self.source, **options)["boards"]}

    def test_only_the_current_board_prefix_is_removed(self):
        for slug, value, expected in [
            ("s4s", OLD_TITLE, SHORT_TITLE),
            ("s4s", "/s4s/ - " + SHORT_TITLE, SHORT_TITLE),
            ("r9k", "/r9k/ - ROBOT9001", "ROBOT9001"),
            ("j", "/j/ - Janitor & Moderator Discussion", "Janitor & Moderator Discussion"),
            ("s4s", "[s4s] - Owned [s4s] - title", "Owned [s4s] - title"),
            ("s4s", "[other] - Custom title", "[other] - Custom title"),
            ("a", "[a] - Custom title", "[a] - Custom title"),
            ("a", OLD_TITLE, OLD_TITLE),
            ("s4s", "Prefix " + OLD_TITLE, "Prefix " + OLD_TITLE),
        ]:
            with self.subTest(slug=slug, value=value):
                self.assertEqual(MODULE.short_title(slug, value), expected)
        self.assertEqual(MODULE.short_title("s4s", OLD_TITLE, historical_titles=True), OLD_TITLE)

    def test_title_override_wins_over_static_directory_name(self):
        self.assertEqual(self.titles(), {"a": "Anime & Manga", "vp": "Pokémon", "s4s": SHORT_TITLE})
        board = MODULE.extract(self.source)["boards"][-1]
        self.assertEqual(board["source_policy"]["TITLE"], OLD_TITLE)

    def test_encoding_and_historical_title_behavior_are_independent(self):
        self.assertEqual(self.titles(names_encoding="cp1252")["s4s"], SHORT_TITLE)
        self.assertEqual(self.titles(names_encoding="cp1252")["vp"], "PokÃ©mon")
        self.assertEqual(self.titles(historical_titles=True)["s4s"], OLD_TITLE)
        self.assertEqual(self.titles(historical_titles=True)["vp"], "Pokémon")

    def test_current_fixture_and_historical_migrations_keep_their_contracts(self):
        boards = {board["slug"]: board for board in REFERENCE["boards"]}
        self.assertEqual(boards["s4s"]["title"], SHORT_TITLE)
        self.assertEqual(boards["s4s"]["source_policy"]["TITLE"], OLD_TITLE)
        historical_utf8 = copy.deepcopy(REFERENCE)
        next(board for board in historical_utf8["boards"] if board["slug"] == "s4s")["title"] = OLD_TITLE
        historical = copy.deepcopy(historical_utf8)
        next(board for board in historical["boards"] if board["slug"] == "vp")["title"] = "PokÃ©mon"
        self.assertEqual(MODULE.migration(historical).encode("utf-8"),
                         (ROOT / "migrations/0045_original_boards.sql").read_bytes())
        self.assertEqual(MODULE.board_encoding_migration(historical_utf8, historical).encode("utf-8"),
                         (ROOT / "migrations/0064_board_reference_encoding.sql").read_bytes())
        self.assertEqual(MODULE.board_title_migration(REFERENCE, historical_utf8).encode("utf-8"),
                         TITLE_MIGRATION.read_bytes())
        for filename, expected in {
            "0045_original_boards.sql": "64a16f3f9a9fe7407d612e606a73d5aba0e9b396299410f5fdecaa319d8d0435",
            "0064_board_reference_encoding.sql": "284642e1fd12f152d090a0dc2139bac12190f560877e1fc30ab2fe406677222e",
        }.items():
            self.assertEqual(hashlib.sha256((ROOT / "migrations" / filename).read_bytes()).hexdigest(), expected)

    def test_cli_generates_and_checks_separate_corrections(self):
        output = Path(self.temporary.name)
        command = [sys.executable, str(SCRIPT), str(self.source), str(output / "reference.json"),
                   "--migration", str(output / "0045.sql"),
                   "--board-encoding-migration", str(output / "0064.sql"),
                   "--board-title-migration", str(output / "0120.sql")]
        subprocess.run(command, check=True, capture_output=True, text=True)
        self.assertEqual((output / "0064.sql").read_bytes(),
                         (ROOT / "migrations/0064_board_reference_encoding.sql").read_bytes())
        self.assertEqual((output / "0120.sql").read_bytes(), TITLE_MIGRATION.read_bytes())
        self.assertIn(OLD_TITLE, (output / "0045.sql").read_text(encoding="utf-8"))
        subprocess.run(command + ["--check"], check=True, capture_output=True, text=True)
        with (output / "0120.sql").open("a", encoding="utf-8") as migration:
            migration.write("-- Unexpected change\n")
        result = subprocess.run(command + ["--check"], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Board title migration differs", result.stderr)

    def test_sql_changes_only_the_exact_imported_title_and_is_idempotent(self):
        # SQLite executes this portable UPDATE without needing a server. The
        # application migration suite separately covers PostgreSQL privileges.
        for original in [OLD_TITLE, SHORT_TITLE, "Operator's <title> & Pokémon", OLD_TITLE + " custom"]:
            with self.subTest(title=original), sqlite3.connect(":memory:") as database:
                database.execute("ATTACH DATABASE ':memory:' AS content")
                database.execute("CREATE TABLE content.boards(slug text PRIMARY KEY, title text, description text)")
                database.executemany("INSERT INTO content.boards VALUES(?,?,?)", [
                    ("s4s", original, "Unchanged description"),
                    ("other", OLD_TITLE, "Other board"),
                    ("vp", "Pokémon", "Unchanged Unicode"),
                ])
                expected = [("other", OLD_TITLE, "Other board"),
                            ("s4s", SHORT_TITLE if original == OLD_TITLE else original, "Unchanged description"),
                            ("vp", "Pokémon", "Unchanged Unicode")]
                for _ in range(2):
                    database.executescript(TITLE_MIGRATION.read_text(encoding="utf-8"))
                    self.assertEqual(database.execute("SELECT * FROM content.boards ORDER BY slug").fetchall(), expected)


if __name__ == "__main__":
    unittest.main()
