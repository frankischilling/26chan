#!/usr/bin/env python3
"""Extract the pinned board/thread and catalog initialization notification contract."""
import argparse
import hashlib
import json
from pathlib import Path

REVISION = '545b7812d1849f7958d914950c91fdbbe38f6b22'
FILES = {
    'catalog.php': '9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946',
    'js/catalog.js': '12f59335953bd013ce892a24186218a31f2e8ad7fe3dbaa54af82f40ca182e5e',
    'js/extension.js': '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31',
    'js/core.js': 'a9ab67bea1f51fcdaaac1a879098a4d5888622008bf7f26f1fe18861ea52aab5',
}
BLOCKS = {
    'catalog_caller': ('catalog.php', b"  document.addEventListener('DOMContentLoaded', function() {", b'</script>'),
    'catalog_settings': ('js/catalog.js', b'  function loadSettings()', b'  function onTeaserChange()'),
    'catalog_load': ('js/catalog.js', b'  self.loadCatalog = function(', b'  function initGlobalMessage()'),
    'catalog_init': ('js/catalog.js', b'  self.init = function()', b'  function showDropDownNav()'),
    'catalog_defaults': ('js/catalog.js', b'  options = {', b'  capcodeMap ='),
    'catalog_dispatch': ('js/catalog.js', b'UA.dispatchEvent = function(', b'var FC = function()'),
    'init': ('js/extension.js', b'Main.init = function()', b'Main.initPersistentNav ='),
    'dispatch': ('js/extension.js', b'UA.dispatchEvent = function(', b'UA.getSelection ='),
    'run': ('js/extension.js', b'Main.run = function()', b'Main.on'),
    'math_startup': ('js/core.js', b'  if (window.math_tags && pageHasMath())', b'  if(navigator.userAgent)'),
}

def extract(source):
    raw = {name: (source / name).read_bytes() for name in FILES}
    for name, digest in FILES.items():
        if hashlib.sha256(raw[name]).hexdigest() != digest:
            raise ValueError(f'Audited source differs: {name}')
    snippets = {}
    for name, (file, begin, end) in BLOCKS.items():
        data = raw[file]
        start = data.index(begin)
        # Main.run contains Main.onclick; use its following function boundary.
        stop = data.index(b'\r\nMain.', start + len(begin)) if name == 'run' else data.index(end, start)
        block = data[start:stop]
        snippets[name] = {'file': file, 'byte_start': start, 'byte_end': stop,
                          'sha256': hashlib.sha256(block).hexdigest(), 'text': block.decode()}
    return {'source_revision': REVISION, 'files': FILES, 'snippets': snippets,
            'scope': 'Board/thread pre-parser and catalog controls-ready MainInit boundaries, event shape and caller ordering.'}

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    encoded = (json.dumps(extract(args.source), indent=2, ensure_ascii=True) + '\n').encode()
    if args.check:
        if args.output.read_bytes() != encoded:
            raise ValueError('MainInit reference fixture differs')
    else:
        args.output.write_bytes(encoded)
