#!/usr/bin/env python3
"""Extract byte-pinned legacy math contracts; never import replacement code."""
import argparse
import hashlib
import json
from pathlib import Path

REVISION = '545b7812d1849f7958d914950c91fdbbe38f6b22'
FILES = {
    'js/core.js': 'a9ab67bea1f51fcdaaac1a879098a4d5888622008bf7f26f1fe18861ea52aab5',
    'js/extension.js': '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31',
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'config/global_config.ini': '8bebeedec119b30559cba4fdccfef416653294cf62118d10288434be99a3034d',
    'config/boards/sci.config.ini': '53cd956f750a4a8923c4d7f7c61d48e432cd4b528346627041816b8c3743de18',
    'config/boards/test.config.ini': '72f1ddef8a63944bfb40e5d2e368c8ffeb42bef4c18b02a04d3638186a98b683',
}
# Exact markers deliberately fail closed when audited source changes.
BLOCKS = {
    'core': ('js/core.js', b'function pageHasMath()', b'captchainterval = null;'),
    'startup': ('js/core.js', b'  if (window.math_tags && pageHasMath())', b'  if(navigator.userAgent)'),
    'dynamic': ('js/extension.js', b'  if (offset) {\r\n    if (Parser.prettify)', b'Parser.parseTrackedReplies ='),
    'preview': ('js/extension.js', b'QR.openTeXPreview =', b'QR.validateCT ='),
    'server_parser': ('imgboard.php', b'function jsmath_parse(', b'/* BBCode for bold'),
    'server_disabled_call': ('imgboard.php', b'  /*\r\n\tif( JSMATH == 1 )', b'\tif( CODE_TAGS )'),
    'page_policy': ('imgboard.php', b'  if (JSMATH) {\r\n    $scriptjs', b'  if (SJIS_TAGS)'),
    'api_policy': ('imgboard.php', b"  if (JSMATH) {\r\n    $arr['math_tags']", b'  if (SHOW_COUNTRY_FLAGS)'),
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
        stop = data.index(end, start)
        block = data[start:stop]
        snippets[name] = {'file': file, 'byte_start': start, 'byte_end': stop,
                          'sha256': hashlib.sha256(block).hexdigest(), 'text': block.decode()}
    policies = {name: [line for line in raw[name].decode().splitlines() if 'JSMATH' in line]
                for name in FILES if name.endswith('.ini')}
    return {'source_revision': REVISION, 'files': FILES, 'snippets': snippets, 'policies': policies,
            'scope': 'Legacy detection, delimiters, safety settings, policy and lifecycle. Not a MathJax renderer oracle.'}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    encoded = (json.dumps(extract(args.source), indent=2, ensure_ascii=True) + '\n').encode()
    if args.check:
        if args.output.read_bytes() != encoded:
            raise ValueError('Math reference fixture differs')
    else:
        args.output.write_bytes(encoded)

if __name__ == '__main__':
    main()
