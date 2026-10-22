#!/usr/bin/env python3
"""Pin the duplicate-comment branch without loading the original application."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
pins = {
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'config/global_config.ini': '8bebeedec119b30559cba4fdccfef416653294cf62118d10288434be99a3034d',
    'config/global_strings.ini': '3b3b60083db74fb1693af6f0824cccd6ec3ca446242474008cf9be474c65cd17',
    'lib/util.php': '5307cea255e2d3eb243c7549e1a6521281bf6253c47e010476412b9a4c63a70c',
}
texts = {}
for name, digest in pins.items():
    raw = (args.source / name).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == digest, name
    texts[name] = raw.decode().replace('\r\n', '\n')
source = texts['imgboard.php']
start = source.index('\tif( !$silent_reject ) {', source.index('//post text is now completely created'))
end = source.index('\n\t\t\t/**', start)
block = source[start:end] + '\n\t\t}\n\t}\n'
util = texts['lib/util.php']
start = util.index('function sec2hms(')
end = util.index('\nfunction flush_output_buffers', start)
result = {
    'source': pins,
    'gate_php': 'function source_duplicate($silent_reject, $com, $host, $time, $dest) {\n' + block + '}\n',
    'duration_php': util[start:end],
    'settings': {name: [line for line in text.splitlines() if line.startswith(('RENZOKU_DUPE =', 'S_RENZOKU_DUP ='))] for name, text in texts.items() if name.startswith('config/')},
    'limits': ['Synthetic rows exercise temporal and role semantics, not undocumented deployed SQL collation.', 'The original application, credentials and network are never loaded.'],
}
encoded = json.dumps(result, indent=2) + '\n'
path = Path('fixtures/comment-duplicate-source.json')
if args.check:
    assert path.read_text() == encoded
else:
    path.write_text(encoded)
