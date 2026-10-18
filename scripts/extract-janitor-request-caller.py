#!/usr/bin/env python3
"""Retain the complete request caller and metadata decoder for isolated tests."""
import argparse
import hashlib
import json
import re
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
inputs = [
    ("admin.php", "4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb", "adminBanReq", 20000),
    ("lib/admin.php", "571d154aa43cd51ee17bb4dfec5502d6dd63c265f4a2526820c011c934c7d185", "decode_user_meta", 2048),
]
record = {}
for filename, digest, function, limit in inputs:
    raw = (args.source / filename).read_bytes()
    if hashlib.sha256(raw).hexdigest() != digest:
        raise SystemExit(f"Audited source differs: {filename}")
    source = raw.decode().replace("\r\n", "\n")
    matches = re.findall(r"^function " + function + r"\([^\n]*\).*?^}", source, re.M | re.S)
    if len(matches) != 1 or len(matches[0]) > limit:
        raise SystemExit(f"Expected one bounded function: {function}")
    record[function] = {"source": filename, "sha256": digest, "php": matches[0]}
result = json.dumps(record, indent=2) + "\n"
output = Path(__file__).resolve().parent.parent / "fixtures/janitor-request-caller-source.json"
if args.check:
    if output.read_text() != result:
        raise SystemExit("Janitor request caller fixture differs")
else:
    output.write_text(result)
print("Janitor request caller and decoder match the pinned source.")
