#!/usr/bin/env python3
"""Evaluate pinned JSON identity blocks and reply grouping with synthetic posts."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    source = (args.source / 'json.php').read_bytes()
    digest = '18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868'
    if hashlib.sha256(source).hexdigest() != digest:
        raise RuntimeError('Audited JSON source differs.')
    text = source.decode('utf8').replace('\r\n', '\n')
    grouping = text[text.index('function generate_capcode_replies('):
                    text.index('function post_json_force_type(')].strip()
    if re.search(r'\b(include|require|eval|file_|mysql|PDO|cookie|session)\b', grouping):
        raise RuntimeError('Reply grouping contains an unexpected dependency.')
    cleanup = text[text.index('\t// clean up names\n'):
                   text.index('\n\tif( !$banskip && SPOILERS')]
    start = text.index('\tif( ( $FORCED_ANON || $META_BOARD )')
    masking = text[start:text.index('\n\tif( $var[\'capcode\'] == \'none\' )', start)]
    if len(grouping.encode()) > 2048 or len(cleanup.encode()) > 512 or len(masking.encode()) > 512:
        raise RuntimeError('JSON helper blocks exceed their audited bounds.')
    program = grouping + '\nfunction project_identity($var,$FORCED_ANON,$META_BOARD) {\n' + cleanup + masking + '''
        $ret=[];
        if (isset($var['name'])) $ret['name']=$var['name'];
        if (isset($var['trip'])) $ret['trip']=$var['trip'];
        return $ret;
    }
    $cases=[];
    foreach ([false,true] as $forced) foreach ([false,true] as $meta)
    foreach (['none','mod','admin','admin_highlight','manager','developer','founder','admin_hl','unknown'] as $badge)
    foreach ([['Owned &amp; &quot;',null],['Owned','!ozOtJW9BFA'],['','!!abcdefghijk']] as [$name,$trip]) {
        $var=['name'=>$trip===null?$name:$name.'</span> <span class="postertrip">'.$trip,'capcode'=>$badge];
        $projected=project_identity($var,$forced,$meta);
        $cases[]=['forced_anonymous'=>$forced,'meta_board'=>$meta,'capcode'=>$badge,
                  'name'=>$name,'trip'=>$trip,'expected'=>$projected];
    }
    $groups=[];
    foreach ([[],['none'],['mod'],['admin_highlight','admin','mod','developer','manager','founder'],
              ['mod','mod','none','admin_highlight'],['founder','developer','manager','admin'],
              ['unknown','admin_hl'],['mod',null,'developer']] as $badges) {
        $log=[];$replies=[];$rows=[];
        foreach ($badges as $i=>$badge) {
            $no=1001+$i;$replies[$no]=true;
            if ($badge!==null) $log[$no]=['capcode'=>$badge];
            $rows[]=['id'=>$no,'capcode'=>$badge];
        }
        $groups[]=['rows'=>$rows,'expected'=>generate_capcode_replies($replies)];
    }
    echo json_encode(['php'=>PHP_VERSION,'identity_cases'=>$cases,'reply_cases'=>$groups],flags:JSON_THROW_ON_ERROR);
    '''
    result = subprocess.run(['php', '-d', 'memory_limit=16M', '-r', program],
                            capture_output=True, timeout=5, check=True)
    if result.stderr or len(result.stdout) > 32_768:
        raise RuntimeError('JSON reference output exceeds its bounds or reports an error.')
    output = json.loads(result.stdout)
    if len(output['identity_cases']) != 108 or len(output['reply_cases']) != 8:
        raise RuntimeError('JSON reference case counts differ.')
    fixture = {'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
               'files': {'json.php': digest}, **output}
    encoded = (json.dumps(fixture, indent=2, ensure_ascii=True) + '\n').encode()
    if len(encoded) > 65_536:
        raise RuntimeError('JSON reference fixture exceeds its bounds.')
    if args.check:
        if args.output.read_bytes() != encoded:
            raise RuntimeError('JSON identity reference differs.')
    else:
        args.output.write_bytes(encoded)
    print(json.dumps({'json_identity_cases': 108, 'json_reply_cases': 8}))


if __name__ == '__main__':
    main()
