"""Bounded fail-closed controls on temporary copies, never production mutations.

The wire generator is exercised with PYTHONOPTIMIZE=0, 1 and 2. The source
recorder and constructor oracle use only the already-vendored pinned source.
These are fixture-integrity controls, not parser/state/cost or runtime proofs.
"""

from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
WIRE = Path("tests/media/fixtures/replay-wire")
RECORDER = Path("apps/media-guest/tests/fixtures/replay")
COST = Path("tests/media/fixtures/replay-cost")
VENDOR = Path("apps/public/vendor/tegaki/0.9.4/tegaki.min.js")
ORACLE = Path("tests/fixtures/record-native-replay-source.mjs")
ORACLE_OUTPUT = Path("tests/fixtures/native-replay-source.json")
HELPER = Path("tests/helpers/pinned-replay-runtime.mjs")
OPTIMIZATIONS = ("0", "1", "2")

# Direct malformed-input controls deliberately bypass fixture pins only inside
# this separate test process. No generator pin or production source is edited.
STRUCTURAL_CONTROLS = r'''
import importlib.util
from pathlib import Path
import struct
import sys
import zlib

spec = importlib.util.spec_from_file_location("wire_fixture", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
source = Path(sys.argv[2]).read_bytes()
body = zlib.decompress(source[12:], wbits=-15)

def changed(value, offset, replacement):
    return value[:offset] + replacement + value[offset + len(replacement):]

def packed(value):
    compressor = zlib.compressobj(wbits=-15)
    return source[:4] + struct.pack(">I", len(value)) + source[8:12] + compressor.compress(value) + compressor.flush()

cases = {
    "short header": source[:11],
    "wrong magic": changed(source, 0, b"X"),
    "wrong version": changed(source, 10, b"\x05"),
    "short declared body": changed(source, 4, struct.pack(">I", 178)),
    "unbounded declared body": changed(source, 4, struct.pack(">I", 0xffffffff)),
    "wrong declared body": changed(source, 4, struct.pack(">I", len(body) + 1)),
    "invalid deflate": source[:12] + b"\x07",
    "truncated deflate": source[:-1],
    "trailing deflate": source + b"x",
    "concatenated deflate": source + source[12:],
    "inflation exceeds declared length": changed(packed(body + b"x" * 4096), 4, struct.pack(">I", len(body))),
    "metadata length": packed(changed(body, 1, b"\x14")),
    "tool count": packed(changed(body, 21, b"\x07")),
    "tool length": packed(changed(body, 22, b"\x12")),
    "tool zero": packed(changed(body, 23, b"\x00")),
    "tool nine": packed(changed(body, 23, b"\x09")),
    "duplicate tool": packed(changed(body, 42, body[23:24])),
    "count below minimum": packed(changed(body, 175, struct.pack(">I", 1))),
    "count above maximum": packed(changed(body, 175, struct.pack(">I", 16385))),
    "truncated event header": packed(changed(changed(body, 175, struct.pack(">I", 3)), 184, b"\x03") + b"\xff"),
    "unknown tag": packed(changed(body, 179, b"\x09")),
    "missing prelude": packed(changed(body, 179, b"\x03")),
    "missing conclusion": packed(changed(body, 184, b"\x03")),
    "repeated prelude": packed(changed(body, 184, b"\x00")),
    "early conclusion": packed(changed(body, 179, b"\xff")),
    "truncated event payload": packed(changed(body, 184, b"\x01")),
    "extra event bytes": packed(body + b"x"),
}
for slot in range(8):
    for field in (10, 11, 12, 18):
        cases[f"noncanonical toggle {slot}/{field}"] = packed(changed(body, 23 + slot * 19 + field, b"\x02"))
for name, invalid in cases.items():
    try:
        module.transcribe(invalid)
    except ValueError:
        continue
    raise RuntimeError("malformed source accepted: " + name)
print(f"rejected {len(cases)} malformed structural inputs")
'''


class ReplayFixtureGuards(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="replay fixture guards ")
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        for relative in (WIRE, RECORDER, COST):
            shutil.copytree(ROOT / relative, self.root / relative)
        for relative in (VENDOR, ORACLE, ORACLE_OUTPUT, HELPER):
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        self.node = shutil.which("node")
        if not self.node:
            self.fail("Node is required for replay fixture controls")

    @contextmanager
    def changed(self, relative, content):
        path = self.root / relative
        original = path.read_bytes()
        path.write_bytes(content)
        try:
            yield
        finally:
            path.write_bytes(original)

    def snapshot(self, directory, pattern):
        return {path.name: path.read_bytes() for path in (self.root / directory).glob(pattern)}

    def run_command(self, command, *, optimize="0", error=None):
        env = dict(os.environ, PYTHONOPTIMIZE=optimize, PYTHONDONTWRITEBYTECODE="1")
        result = subprocess.run(command, cwd=self.root, env=env, capture_output=True,
                                text=True, timeout=15, check=False)
        output = result.stdout + result.stderr
        if error is None:
            self.assertEqual(result.returncode, 0, output)
        else:
            self.assertNotEqual(result.returncode, 0, output)
            self.assertIn(error, output)
        return output

    def wire(self, *arguments, **options):
        return self.run_command([sys.executable, "-B", str(self.root / WIRE / "generate.py"), *arguments], **options)

    def recorder(self, *arguments, **options):
        return self.run_command([self.node, str(self.root / RECORDER / "generate.cjs"), *arguments], **options)

    def oracle(self, *arguments, **options):
        return self.run_command([self.node, str(self.root / ORACLE), *arguments], **options)

    def test_wire_verify_and_record_are_exact_under_all_optimization_modes(self):
        expected = self.snapshot(WIRE, "*.ibr")
        for optimize in OPTIMIZATIONS:
            with self.subTest(optimize=optimize):
                self.wire(optimize=optimize)
                for name in expected:
                    (self.root / WIRE / name).unlink()
                self.wire("--record", optimize=optimize)
                self.assertEqual(self.snapshot(WIRE, "*.ibr"), expected)
                self.wire(optimize=optimize)

    def test_wire_input_pins_reject_size_and_same_length_changes_before_any_write(self):
        for optimize in OPTIMIZATIONS:
            for name in ("empty", "commands"):
                relative = RECORDER / (name + ".tgkr")
                original = (self.root / relative).read_bytes()
                for mutation in (original + b"x", bytes([original[0] ^ 1]) + original[1:]):
                    with self.subTest(optimize=optimize, name=name, size=len(mutation)):
                        with self.changed(relative, mutation), self.changed(WIRE / "empty.ibr", b"unchanged sentinel"):
                            before = self.snapshot(WIRE, "*.ibr")
                            for args in ([], ["--record"]):
                                self.wire(*args, optimize=optimize, error="input pin mismatch: " + name)
                                self.assertEqual(self.snapshot(WIRE, "*.ibr"), before)

    def test_wire_frozen_drift_rejects_and_only_explicit_record_repairs(self):
        for optimize in OPTIMIZATIONS:
            for name in ("empty", "commands"):
                relative = WIRE / (name + ".ibr")
                original = (self.root / relative).read_bytes()
                with self.subTest(optimize=optimize, name=name):
                    with self.changed(relative, bytes([original[0] ^ 1]) + original[1:]):
                        before = self.snapshot(WIRE, "*.ibr")
                        self.wire(optimize=optimize, error="frozen output differs: " + name)
                        self.assertEqual(self.snapshot(WIRE, "*.ibr"), before)
                        self.wire("--record", optimize=optimize)
                        self.assertEqual((self.root / relative).read_bytes(), original)

    def test_wire_malformed_cli_rejects_before_read_or_write(self):
        for optimize in OPTIMIZATIONS:
            for args in (["--unknown"], ["--record", "--unknown"], ["--record", "--record"],
                         ["unexpected"], ["--record=1"]):
                with self.subTest(optimize=optimize, args=args):
                    with self.changed(RECORDER / "empty.tgkr", b"unreadable input pin"):
                        before = self.snapshot(WIRE, "*.ibr")
                        self.wire(*args, optimize=optimize, error="usage: generate.py [--record]")
                        self.assertEqual(self.snapshot(WIRE, "*.ibr"), before)

    def test_wire_structural_checks_survive_all_optimization_modes(self):
        for optimize in OPTIMIZATIONS:
            with self.subTest(optimize=optimize):
                output = self.run_command([sys.executable, "-B", "-c", STRUCTURAL_CONTROLS,
                    str(self.root / WIRE / "generate.py"), str(self.root / RECORDER / "empty.tgkr")], optimize=optimize)
                self.assertIn("rejected 59 malformed structural inputs", output)

    def test_recorder_vendored_explicit_and_legacy_sources_match(self):
        expected = self.snapshot(RECORDER, "*.tgkr") | self.snapshot(RECORDER, "*.json")
        legacy = self.root / "reference with spaces"
        (legacy / "js").mkdir(parents=True)
        shutil.copyfile(self.root / VENDOR, legacy / "js/tegaki.min.js")
        for args in ([], ["--source-file", str(self.root / VENDOR)], [str(legacy)]):
            with self.subTest(args=args):
                self.recorder(*args)
                for name in expected:
                    (self.root / RECORDER / name).write_bytes(b"deliberate output drift")
                self.recorder(*args, "--record")
                self.assertEqual(self.snapshot(RECORDER, "*.tgkr") | self.snapshot(RECORDER, "*.json"), expected)

    def test_recorder_source_pins_fail_before_execution_or_record(self):
        original = (self.root / VENDOR).read_bytes()
        for mutation, error in ((original + b"x", "production source size changed"),
                                (b"!" + original[1:], "production source hash changed")):
            with self.subTest(error=error), self.changed(VENDOR, mutation):
                before = self.snapshot(RECORDER, "*.tgkr") | self.snapshot(RECORDER, "*.json")
                for args in ([], ["--record"], ["--source-file", str(self.root / VENDOR), "--record"]):
                    self.recorder(*args, error=error)
                    self.assertEqual(self.snapshot(RECORDER, "*.tgkr") | self.snapshot(RECORDER, "*.json"), before)

    def test_recorder_cli_and_frozen_outputs_fail_closed(self):
        for args in (["--unknown"], ["--record", "--unknown"], ["--record", "--record"],
                     ["--source-file"], ["--source-file", "--record"],
                     ["--source-file", "missing", "extra"], ["missing", "extra"],
                     ["missing", "--source-file", "other"],
                     ["--source-file", "missing", "--source-file", "other"]):
            with self.subTest(args=args):
                before = self.snapshot(RECORDER, "*.tgkr") | self.snapshot(RECORDER, "*.json")
                self.recorder(*args, error="usage: generate.cjs")
                self.assertEqual(self.snapshot(RECORDER, "*.tgkr") | self.snapshot(RECORDER, "*.json"), before)
        for name in ("empty.tgkr", "commands.tgkr", "manifest.json"):
            with self.subTest(name=name), self.changed(RECORDER / name, b"frozen drift"):
                self.recorder(error="fixture changed: " + name)
                self.assertEqual((self.root / RECORDER / name).read_bytes(), b"frozen drift")

    def test_constructor_oracle_cli_pin_output_and_record_guards(self):
        expected = (self.root / ORACLE_OUTPUT).read_bytes()
        self.oracle()
        for args in (["--unknown"], ["--record", "--unknown"], ["--record", "--record"], ["unexpected"]):
            with self.subTest(args=args), self.changed(VENDOR, b"must not execute"):
                self.oracle(*args, error="usage: record-native-replay-source.mjs")
                self.assertEqual((self.root / ORACLE_OUTPUT).read_bytes(), expected)
        source = (self.root / VENDOR).read_bytes()
        for mutation in (source + b"x", b"!" + source[1:]):
            with self.subTest(size=len(mutation)), self.changed(VENDOR, mutation):
                for args in ([], ["--record"]):
                    self.oracle(*args, error="AssertionError")
                    self.assertEqual((self.root / ORACLE_OUTPUT).read_bytes(), expected)
        with self.changed(ORACLE_OUTPUT, b"frozen drift"):
            self.oracle(error="Frozen source oracle changed")
            self.assertEqual((self.root / ORACLE_OUTPUT).read_bytes(), b"frozen drift")
            self.oracle("--record")
            self.assertEqual((self.root / ORACLE_OUTPUT).read_bytes(), expected)

    def test_cost_extraction_guards_survive_all_optimization_modes(self):
        script = [sys.executable, "-B", str(self.root / COST / "generate.py")]
        expected = self.snapshot(COST, "*.tsv") | self.snapshot(COST, "*.txt")
        source_path = COST / "source/shapes.json"
        original = (self.root / source_path).read_bytes()
        for optimize in OPTIMIZATIONS:
            with self.subTest(optimize=optimize):
                self.run_command(script, optimize=optimize)
                with self.changed(source_path, b"!" + original[1:]):
                    for args in ([], ["--record"]):
                        self.run_command(script + args, optimize=optimize, error="input pin mismatch: shapes")
                        self.assertEqual(self.snapshot(COST, "*.tsv") | self.snapshot(COST, "*.txt"), expected)
                with self.changed(COST / "shapes.tsv", b"frozen drift"):
                    self.run_command(script, optimize=optimize, error="frozen output differs: shapes.tsv")
                    self.assertEqual((self.root / COST / "shapes.tsv").read_bytes(), b"frozen drift")
                    self.run_command(script + ["--record", "--unknown"], optimize=optimize, error="usage: generate.py")
                    self.assertEqual((self.root / COST / "shapes.tsv").read_bytes(), b"frozen drift")
                    self.run_command(script + ["--record"], optimize=optimize)
                    self.assertEqual(self.snapshot(COST, "*.tsv") | self.snapshot(COST, "*.txt"), expected)


if __name__ == "__main__":
    unittest.main()
