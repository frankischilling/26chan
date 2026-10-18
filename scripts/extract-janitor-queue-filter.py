#!/usr/bin/env python3
"""Check the isolated source queue filter without loading the source application."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
args = parser.parse_args()
record = json.loads(Path('fixtures/janitor-queue-filter.json').read_text())
raw = (args.source / record['source']).read_bytes()
if hashlib.sha256(raw).hexdigest() != record['sha256']:
    raise SystemExit('Pinned ban-request client source differs')
text = raw.decode().replace('\r\n', '\n')
start = 'BR.applySearch = function(str) {'
end = 'BR.refreshRequests = function()'
if text.count(start) != 1 or text.count(end) != 1:
    raise SystemExit('Source filter boundaries are ambiguous')
fragment = start + text.split(start, 1)[1].split(end, 1)[0]
if fragment.rstrip() != record['functions'] or len(fragment) > 4096:
    raise SystemExit('Isolated source filter differs')
print('Pinned request-queue source filter matches.')
