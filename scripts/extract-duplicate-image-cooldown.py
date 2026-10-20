#!/usr/bin/env python3
"""Extract the supplied duplicate-image timing fragment without executing SQL."""
import argparse
import hashlib
import json
from pathlib import Path
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('source', type=Path)
p.add_argument('--check', action='store_true')
a = p.parse_args()
raw = (a.source / 'imgboard.php').read_bytes()
source = raw.decode().replace('\r\n', '\n')
start = source.index('            $cooldown = RENZOKU_DUPE;')
end = source.index('\n          }', start)
fragment = source[start:end].strip()
config = (a.source / 'config/global_config.ini').read_text()
assert '\nRENZOKU_DUPE = 300\n' in config
for path in (a.source / 'config').rglob('*.ini'):
    if path.name != 'global_config.ini':
        assert not any(line.lstrip().startswith('RENZOKU_DUPE =') for line in path.read_text().splitlines()), path
record = {'source_sha256': hashlib.sha256(raw).hexdigest(), 'php': fragment, 'seconds': 300,
          'query': "SELECT time FROM `%s` WHERE host = '%s' AND md5 = '%s' AND resto != %d ORDER BY no DESC LIMIT 1",
          'boundaries': ['Owned last-used timestamps replace SQL results', 'Duration formatting returns seconds', 'Errors are captured instead of sending HTTP']}
assert record['query'] in source
path = Path('fixtures/duplicate-image-cooldown-source.json')
encoded = json.dumps(record, indent=2) + '\n'
if a.check:
    assert path.read_text() == encoded
else:
    path.write_text(encoded)
