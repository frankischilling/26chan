#!/usr/bin/env python3
"""Record the supplied source's post-admission autoarchive predicate."""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import re

SOURCE_HASH = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'


def generate(root):
    raw = (root / 'imgboard.php').read_bytes()
    if hashlib.sha256(raw).hexdigest() != SOURCE_HASH:
        raise ValueError('Reference imgboard.php changed')
    source = raw.decode().replace('\r\n', '\n')
    start = source.index('    if (AUTOARCHIVE_CAP && $resto && !$sticky && !$undead && ENABLE_ARCHIVE)')
    block = source[start:source.index('// update html', start)]
    assert 'count_thread_replies(BOARD_DIR, $resto) >= AUTOARCHIVE_CAP' in block
    assert 'archive_thread($resto);' in block
    counter = source[source.index('function count_thread_replies('):]
    assert 'SELECT COUNT(*) as cnt FROM `%s` WHERE resto = $thread_id' in counter
    policies = {}
    global_config = (root / 'config/global_config.ini').read_text()
    default = int(re.search(r'^AUTOARCHIVE_CAP\s*=\s*(\d+)', global_config, re.M)[1])
    assert default == 0
    for path in sorted((root / 'config/boards').glob('*.config.ini')):
        match = re.search(r'^AUTOARCHIVE_CAP\s*=\s*(\d+)', path.read_text(), re.M)
        if match:
            policies[path.name.removesuffix('.config.ini')] = int(match[1])
    assert policies == {'jp': 1500, 'vt': 1500}
    rows = []
    for cap, reply, sticky, undead, enabled, count in itertools.product(
        (0, 1, 3, 1500), (False, True), (False, True), (False, True), (False, True), (0, 1, 2, 3, 1499, 1500, 1501)
    ):
        rows.append(dict(cap=cap, reply=reply, sticky=sticky, undead=undead,
                         enabled=enabled, live_replies=count,
                         archive=bool(cap and reply and not sticky and not undead and enabled and count >= cap)))
    return {'source_sha256': SOURCE_HASH, 'predicate_sha256': hashlib.sha256(block.encode()).hexdigest(),
            'default_cap': default, 'board_caps': policies, 'cases': rows}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=Path('fixtures/autoarchive-reference.json'))
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    encoded = json.dumps(generate(args.source), indent=2) + '\n'
    if args.check:
        if args.output.read_text() != encoded:
            raise SystemExit('Autoarchive reference differs')
    else:
        args.output.write_text(encoded)
    print('Autoarchive source predicate and board thresholds match.')


if __name__ == '__main__':
    main()
