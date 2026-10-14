#!/usr/bin/env python3
"""Verify the release-owned WebM fallback and its redistribution notices."""
import hashlib
import json
from pathlib import Path
ROOT=Path(__file__).resolve().parent.parent
manifest=json.loads((ROOT/'docs/third-party/ogv/provenance.json').read_text())
expected=set()
for name,entry in manifest['files'].items():
    path=ROOT/name
    assert path.is_file() and not path.is_symlink(),name
    data=path.read_bytes()
    assert len(data)==entry['bytes'] and hashlib.sha256(data).hexdigest()==entry['sha256'],name
    if name.startswith('apps/public/static/ogv/'):
        expected.add(path.name)
        assert 0 < len(data) < 262144
        if path.suffix=='.wasm': assert data.startswith(b'\0asm')
assert {p.name for p in (ROOT/'apps/public/static/ogv').iterdir()}==expected
assert len(expected)==13
print('Pinned OGV WebM assets and notices match the upstream release.')
