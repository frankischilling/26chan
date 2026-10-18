#!/usr/bin/env python3
"""Extract bounded review branches without loading the source application."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = (args.source / "reports/ReportQueue.php").read_bytes()
digest = hashlib.sha256(source).hexdigest()
if digest != "e1eaa54bd229f0d718d026cb11a757576f67f02a86e21c4bf5ff051bbfbac0d1":
    raise SystemExit("Audited report queue source differs")
text = source.decode().replace("\r\n", "\n")
method = text.split("  public function accept_ban_request() {", 1)[1].split(
    "  public function deny_ban_request() {", 1)[0]

def block(start, end):
    return start + method.split(start, 1)[1].split(end, 1)[0]

blocks = {
    "amendment": block("    $amend_length = false;", "    // Don't allow pass revoke templates"),
    "duration_scope": block("    if ($warn) {\n      $length = date", "    // Delete the file if the request was amended"),
    "escalation": block("      // Check if ban should be changed to global", "      // FIXME: email field"),
    "duplicates": block("    if ($template['save_post'] != 'json_only') {", '      $query = "SELECT id FROM ban_requests') + "    }\n",
}
result = json.dumps({
    "source": "reports/ReportQueue.php:accept_ban_request",
    "sha256": digest,
    "scope": "Amendment, duration, scope, escalation and duplicate-selection branches. A closing brace terminates the duplicate predicate before its SQL call. Database writes, role checks and private configuration are not executed.",
    "php": blocks,
}, indent=2) + "\n"
output = Path(__file__).resolve().parent.parent / "fixtures/ban-request-review-source.json"
if args.check:
    if output.read_text() != result:
        raise SystemExit("Ban-request review source fixture differs")
else:
    output.write_text(result)
print("Ban-request review branches match the pinned source.")
