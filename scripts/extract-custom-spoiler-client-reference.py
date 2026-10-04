"""Record the pinned native cache and catalog suffix expressions without a browser."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
pins = {
    'js/extension.js': '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31',
    'js/catalog.js': '12f59335953bd013ce892a24186218a31f2e8ad7fe3dbaa54af82f40ca182e5e',
}
texts = {}
for name, expected in pins.items():
    data = (args.source / name).read_bytes()
    assert hashlib.sha256(data).hexdigest() == expected, name
    texts[name] = data.decode()
extension, catalog = texts['js/extension.js'], texts['js/catalog.js']
native = extension[extension.index('Parser.setCustomSpoiler = function('):extension.index('Parser.buildPost = function(')].strip()
start = catalog.index('    if (catalog.custom_spoiler) {')
suffix = catalog[start:catalog.index("    html = '';", start)].strip()
assert len(native) < 600 and len(suffix) < 300
fixture = {
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': pins,
    'scope': 'source native per-board cache function and catalog suffix expression',
    'boundary_stubs': ['document image lookup', 'current board', 'controlled Math.random', 'catalog metadata'],
    'excludes': ['complete source browser extension', 'historical CDN pixel identity'],
    'native_selection': native,
    'catalog_selection': suffix,
}
encoded = (json.dumps(fixture, indent=2) + '\n').encode()
if args.check:
    assert args.output.read_bytes() == encoded, 'Source spoiler client reference changed'
else:
    args.output.write_bytes(encoded)
print('Pinned native spoiler cache and catalog suffix reference verified.')
