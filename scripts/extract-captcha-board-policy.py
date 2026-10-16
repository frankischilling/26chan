#!/usr/bin/env python3
"""Extract only the five public CAPTCHA switches from source INI files."""
import argparse
import hashlib
import json
import re
from pathlib import Path

KEYS = {
    'CAPTCHA': 'enabled',
    'CAPTCHA_TWISTER': 'twister',
    'CAPTCHA_ALLOW_BYPASS': 'allow_bypass',
    'PREUPLOAD_CAPTCHA': 'preupload',
    'NOSCRIPT_CAPTCHA_ONLY': 'noscript_only',
}
NAME = re.compile(r'[a-z0-9]{1,10}\Z')


def fields(path):
    result = {}
    for line in path.read_text(encoding='utf-8').splitlines():
        match = re.match(r'^\s*([A-Z_]+)\s*=\s*(.*?)\s*$', line)
        if match and match[1] in (*KEYS, 'CATEGORY'):
            key, value = match.groups()
            if key in result:
                raise ValueError('Duplicate public CAPTCHA policy field')
            if key == 'CATEGORY':
                if value and not NAME.fullmatch(value):
                    raise ValueError('Invalid category name')
                result[key] = value
            else:
                if value.lower() not in ('yes', 'no'):
                    raise ValueError('Invalid CAPTCHA switch')
                result[key] = value.lower() == 'yes'
    return result


def capture(root):
    listing = root / 'boardlist.txt'
    slugs = listing.read_text(encoding='utf-8').split()
    if len(slugs) != len(set(slugs)) or any(not NAME.fullmatch(s) for s in slugs):
        raise ValueError('Invalid board list')
    extra = {p.name.removesuffix('.config.ini') for p in (root / 'config/boards').glob('*.config.ini')}
    if any(not NAME.fullmatch(s) for s in extra):
        raise ValueError('Invalid board filename')
    global_path = root / 'config/global_config.ini'
    defaults = fields(global_path)
    if not set(KEYS).issubset(defaults):
        raise ValueError('Missing global CAPTCHA switch')
    paths = {listing, global_path}
    rows = []
    for slug in slugs + sorted(extra - set(slugs)):
        board_path = root / f'config/boards/{slug}.config.ini'
        board = fields(board_path)
        if not board.get('CATEGORY'):
            raise ValueError('Missing board category')
        category_path = root / f'config/categories/{board["CATEGORY"]}.config.ini'
        effective = defaults | fields(category_path) | board
        paths.update((category_path, board_path))
        rows.append({'slug': slug, **{column: effective[key] for key, column in KEYS.items()}})
    return {
        'files': {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
                  for path in sorted(paths)},
        'boards': rows,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    encoded = json.dumps(capture(args.source), indent=2) + '\n'
    if args.check:
        if args.output.read_text(encoding='utf-8') != encoded:
            raise ValueError('CAPTCHA board policy differs from source')
    else:
        args.output.write_text(encoded, encoding='utf-8')


if __name__ == '__main__':
    main()
