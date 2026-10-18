#!/usr/bin/env python3
"""Retain the isolated range-production caller from the supplied snapshot."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = (args.source / "admin.php").read_bytes()
digest = hashlib.sha256(source).hexdigest()
if digest != "4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb":
    raise SystemExit("Audited source differs")
text = source.decode("utf-8").replace("\r\n", "\n")
start = text.index("  if (!$warn && $_post_meta && $_post_meta['is_mobile']) { // mobile devices only",
                   text.index("// Auto-rangebans processing (bans)"))
end = text.index("    // Collect and ban other IPs", start)
snippet = text[start:end].rstrip() + "\n  }\n"
result = json.dumps({
    "source": "admin.php:2961-2965",
    "sha256": digest,
    "scope": "Selected non-warning/mobile/template-rule branch; appended closing brace excludes unrelated commented and subsequent actions.",
    "php": snippet,
}, indent=2) + "\n"
output = Path(__file__).resolve().parent.parent / "fixtures/template-range-caller-reference.json"
if args.check:
    if output.read_text() != result:
        raise SystemExit("Template range caller fixture differs")
else:
    output.write_text(result)
print("Template range caller matches the pinned source.")
