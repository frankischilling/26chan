#!/usr/bin/env python3
"""Capture pinned format-metadata clauses without executing source PHP.

The four expected objects are an independently enumerated truth table. This
extractor verifies the exact source bytes, not a second implementation of Rust.
Use --source PATH to verify the original checkout, --write to regenerate.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/board-format-metadata-reference.json'
REVISION = '545b7812d1849f7958d914950c91fdbbe38f6b22'
SOURCE_HASH = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
BLOCK_HASH = '62aebcec14394ad06dbfd7ca65bc55dba80be986767c23dcaeeab7cb7840f427'
BLOCK = "  if (CODE_TAGS) {\r\n    $arr['code_tags'] = 1;\r\n  }\r\n  \r\n  if (SJIS_TAGS) {\r\n    $arr['sjis_tags'] = 1;\r\n  }\r\n"
CASES = [
    {'code_tags': False, 'sjis_tags': False, 'expected': {}},
    {'code_tags': False, 'sjis_tags': True, 'expected': {'sjis_tags': 1}},
    {'code_tags': True, 'sjis_tags': False, 'expected': {'code_tags': 1}},
    {'code_tags': True, 'sjis_tags': True, 'expected': {'code_tags': 1, 'sjis_tags': 1}},
]


def capture(source=None):
    if source is not None:
        data = (source / 'imgboard.php').read_bytes()
        if hashlib.sha256(data).hexdigest() != SOURCE_HASH:
            raise ValueError('Source hash mismatch')
        if b''.join(data.splitlines(keepends=True)[7882:7889]) != BLOCK.encode():
            raise ValueError('Source excerpt mismatch')
    if hashlib.sha256(BLOCK.encode()).hexdigest() != BLOCK_HASH:
        raise ValueError('Excerpt hash mismatch')
    return {
        'source_revision': REVISION,
        'files': {'imgboard.php': SOURCE_HASH},
        'source_excerpt': {'function': 'get_catalog_info', 'first_line': 7883,
                           'last_line': 7889, 'sha256': BLOCK_HASH, 'text': BLOCK},
        'stored_columns': {'CODE_TAGS': 'comment_code_spacing', 'SJIS_TAGS': 'comment_sjis_spacing'},
        'cases': CASES,
    }


def encoded(source=None):
    return (json.dumps(capture(source), indent=2) + '\n').encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    expected = encoded(args.source)
    if args.write:
        FIXTURE.write_bytes(expected)
    elif FIXTURE.read_bytes() != expected:
        raise ValueError('Board format metadata fixture differs')
    print('Board format metadata: pinned excerpt and four cases verified')


if __name__ == '__main__':
    main()
