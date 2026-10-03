#!/usr/bin/env python3
"""Evaluate the pinned pure JSON ID projection with synthetic saved labels."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path)
    parser.add_argument('display',type=Path)
    parser.add_argument('output',type=Path)
    parser.add_argument('--check',action='store_true')
    args=parser.parse_args()
    source=(args.source/'json.php').read_bytes()
    digest='18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868'
    if hashlib.sha256(source).hexdigest()!=digest:
        raise RuntimeError('Audited JSON source differs.')
    text=source.decode('utf8').replace('\r\n','\n')
    start=text.index("\tif ( !$banskip ) {\n\t\tif( !$var['id'] || $is_archived ) {")
    block=text[start:text.index('\n\t}',start)+len('\n\t}')]
    if len(block.encode())>512:
        raise RuntimeError('JSON ID helper exceeds its audited shape.')
    if args.display.stat().st_size>32768:
        raise RuntimeError('Saved display reference exceeds its bound.')
    display=json.loads(args.display.read_text())
    if len(display['cases'])!=112:
        raise RuntimeError('Saved display reference is incomplete.')
    program='''
function generate_uid($no,$time,$host) { return 'Ab12+/CD'; }
function project($saved,$capcode,$op,$is_archived) {
    $banskip=false; $threadid=42; $intern_host='192.0.2.10';
    $var=['id'=>$saved,'capcode'=>$capcode,'no'=>$op?42:43,'time'=>1];
BLOCK
    return $var['id']??null;
}
$inputs=json_decode(stream_get_contents(STDIN),true,flags:JSON_THROW_ON_ERROR);
$cases=[];
foreach($inputs as $index=>$row)
foreach([false,true] as $op)
foreach([false,true] as $archived)
    $cases[]=['display_case'=>$index,'op'=>$op,'archived'=>$archived,
              'expected'=>project($row['expected'],$row['capcode'],$op,$archived)];
echo json_encode(['php'=>PHP_VERSION,'cases'=>$cases],flags:JSON_THROW_ON_ERROR);
'''.replace('BLOCK',block)
    result=subprocess.run(['php','-d','memory_limit=16M','-r',program],
                          input=json.dumps(display['cases']).encode(),capture_output=True,check=True,timeout=5)
    if result.stderr or len(result.stdout)>65536:
        raise RuntimeError('JSON ID output exceeds its bound or reports an error.')
    result=json.loads(result.stdout)
    if len(result['cases'])!=448:
        raise RuntimeError('JSON ID reference case count differs.')
    fixture={'source_revision':display['source_revision'],'files':{'json.php':digest},
             'network_label_stub':'Ab12+/CD',**result}
    encoded=(json.dumps(fixture,indent=2,ensure_ascii=True)+'\n').encode()
    if len(encoded)>65536:
        raise RuntimeError('JSON ID fixture exceeds its bound.')
    if args.check:
        if args.output.read_bytes()!=encoded:
            raise RuntimeError('JSON ID reference differs.')
    else:
        args.output.write_bytes(encoded)
    print(json.dumps({'source_poster_id_json_cases':448}))


if __name__=='__main__':
    main()
