#!/usr/bin/env python3
"""Evaluate the pinned pure static staff ID block with synthetic badge values."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    source = (args.source / 'imgboard.php').read_bytes()
    digest = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
    if hashlib.sha256(source).hexdigest() != digest:
        raise RuntimeError('Audited posting source differs.')
    text = source.decode('utf8').replace('\r\n', '\n')
    start = text.index("\t\t$ma      = ( $capcode == 'admin_highlight') ? 'admin' : $capcode;")
    block = text[start:text.index('\n\t\tif( $ma !=', start)]
    if len(block.encode()) > 512 or block.count('DISP_ID') != 1:
        raise RuntimeError('Static ID helper exceeds its audited shape.')
    # Parameterize only the display switch; the source badge mapping is intact.
    block = block.replace('DISP_ID', '$display_ids')
    program = 'function project($capcode,$display_ids) {$uid=null;\n' + block + '''
        return $uid;
    }
    $rows=[];
    foreach ([false,true] as $enabled)
    foreach (['none','mod','admin','admin_highlight','manager','developer','founder'] as $badge)
        $rows[]=['capcode'=>$badge,'display_ids'=>$enabled,'expected'=>project($badge,$enabled)];
    echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],flags:JSON_THROW_ON_ERROR);
    '''
    result = subprocess.run(['php', '-d', 'memory_limit=16M', '-r', program],
                            check=True, capture_output=True, timeout=5)
    if result.stderr or len(result.stdout) > 4096:
        raise RuntimeError('Static ID output exceeds its bounds or reports an error.')
    output = json.loads(result.stdout)
    if len(output['cases']) != 14:
        raise RuntimeError('Static ID reference case count differs.')
    fixture = {'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
               'files': {'imgboard.php': digest}, **output}
    encoded = (json.dumps(fixture, indent=2, ensure_ascii=True) + '\n').encode()
    if len(encoded) > 8192:
        raise RuntimeError('Static ID fixture exceeds its bound.')
    if args.check:
        if args.output.read_bytes() != encoded:
            raise RuntimeError('Static staff ID reference differs.')
    else:
        args.output.write_bytes(encoded)
    print(json.dumps({'static_staff_id_cases': 14}))


if __name__ == '__main__':
    main()
