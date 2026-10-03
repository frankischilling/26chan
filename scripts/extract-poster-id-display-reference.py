#!/usr/bin/env python3
"""Evaluate the pinned pure poster-ID display blocks without application code."""
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
    staff = text[start:text.index('\n\t\tif( $ma !=', start)]
    start = text.index('\tif( DISP_ID == 1 && !$uid ) {')
    fallback = text[start:text.index('\n\t}', start) + len('\n\t}')]
    if len(staff.encode()) > 512 or len(fallback.encode()) > 256:
        raise RuntimeError('Poster-ID blocks exceed their audited shape.')
    blocks = staff + '\n' + fallback
    for name, parameter in [('DISP_ID_NO_HEAVEN', '$no_heaven'), ('META_BOARD', '$meta'), ('DISP_ID', '$enabled')]:
        blocks = blocks.replace(name, parameter)
    program = '''
function generate_uid($thread, $time) { return 'Ab12+/CD'; }
function project($capcode, $enabled, $is_sage, $meta, $no_heaven) {
    $uid=null; $resto=42; $time=1;
BLOCKS
    return $uid;
}
$cases=[];
foreach ([false,true] as $enabled)
foreach ([false,true] as $sage)
foreach ([false,true] as $meta)
foreach ([false,true] as $no_heaven)
foreach (['none','mod','admin','admin_highlight','manager','developer','founder'] as $capcode)
    $cases[]=['enabled'=>$enabled,'sage'=>$sage,'meta_board'=>$meta,'no_heaven'=>$no_heaven,
              'capcode'=>$capcode,'expected'=>project($capcode,$enabled,$sage,$meta,$no_heaven)];
echo json_encode(['php'=>PHP_VERSION,'cases'=>$cases],flags:JSON_THROW_ON_ERROR);
'''.replace('BLOCKS', blocks)
    result = subprocess.run(['php', '-d', 'memory_limit=16M', '-r', program],
                            check=True, capture_output=True, timeout=5)
    if result.stderr or len(result.stdout) > 16384:
        raise RuntimeError('Poster-ID reference output exceeds its bound or reports an error.')
    result = json.loads(result.stdout)
    if len(result['cases']) != 112:
        raise RuntimeError('Poster-ID reference case count differs.')
    fixture = {'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
               'files': {'imgboard.php': digest}, 'network_label_stub': 'Ab12+/CD', **result}
    encoded = (json.dumps(fixture, indent=2, ensure_ascii=True) + '\n').encode()
    if len(encoded) > 32768:
        raise RuntimeError('Poster-ID display fixture exceeds its bound.')
    if args.check:
        if args.output.read_bytes() != encoded:
            raise RuntimeError('Poster-ID display reference differs.')
    else:
        args.output.write_bytes(encoded)
    print(json.dumps({'source_poster_id_display_cases': 112}))


if __name__ == '__main__':
    main()
