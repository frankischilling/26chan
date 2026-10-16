#!/usr/bin/env python3
"""Run only the pinned final-content admission block with synthetic inputs."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--php", default="php")
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = (args.source / "imgboard.php").read_bytes()
expected = "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
if hashlib.sha256(source).hexdigest() != expected:
    raise SystemExit("Pinned imgboard.php differs")
text = source.decode("utf-8")
start = text.index("  if (!$resto) { // new threads require subject or comment")
end = text.index("\tif( WORD_FILT", start)
block = text[start:end]
if len(block) > 2048:
    raise SystemExit("Selected final-admission block exceeds bounds")
worker = """<?php
error_reporting(E_ALL);
class OwnedAdmissionFailure extends Exception {}
function error($message, $destination) { throw new OwnedAdmissionFailure($message); }
function has_level() { return $GLOBALS['owned_moderator']; }
define('S_NOTEXT_OP', 'op_text');
define('S_NOSUB', 'subject');
define('S_NOTEXT', 'reply_text');
"""
function = "function owned_admit($resto,$sub,$com,$has_image,$options_field) { $dest='owned';\n" + block + "\n}\n"
loops = """
$cases = [];
foreach ([0,1] as $resto) foreach (['','subject'] as $sub)
foreach (['',' ','text'] as $com) foreach ([false,true] as $has_image)
foreach (['','nonoko'] as $options_field) foreach ([false,true] as $moderator) {
    $GLOBALS['owned_moderator'] = $moderator;
    try { owned_admit($resto,$sub,$com,$has_image,$options_field); $outcome='allow'; }
    catch (OwnedAdmissionFailure $error) { $outcome=$error->getMessage(); }
    $cases[] = compact('resto','sub','com','has_image','options_field','moderator','outcome');
}
echo json_encode($cases, JSON_THROW_ON_ERROR);
"""
cases = []
for text_only in (False, True):
    program = worker + "define('TEXT_ONLY', " + str(text_only).lower() + ");\n" + function + loops
    result = subprocess.run(
        [args.php, "-d", "memory_limit=32M", "-d", "max_execution_time=5"],
        input=program, text=True, capture_output=True, timeout=10, check=True,
    )
    if result.stderr or len(result.stdout) > 65536:
        raise SystemExit("Synthetic admission worker exceeded its output contract")
    batch = json.loads(result.stdout)
    if len(batch) != 96:
        raise SystemExit("Synthetic admission case set differs")
    cases.extend(dict(case, text_only=text_only) for case in batch)
output = json.dumps({
    "source": "imgboard.php",
    "sha256": expected,
    "scope": "Final content admission after formatting; early required subject and upload policy are separate",
    "boundary_stubs": ["moderator decision", "terminal error"],
    "cases": cases,
}, ensure_ascii=False, indent=2) + "\n"
if args.check:
    if args.output.read_text() != output:
        raise SystemExit("Admission fixture differs")
else:
    args.output.write_text(output)
print(f"Source final-admission cases: {len(cases)}")
