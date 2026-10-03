#!/usr/bin/env python3
"""Evaluate only the two pinned catalog identity predicates with synthetic values."""
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
    source = (args.source / 'catalog.php').read_bytes()
    digest = '9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946'
    if hashlib.sha256(source).hexdigest() != digest:
        raise RuntimeError('Audited catalog source differs.')
    expressions = re.findall(rb'^ {2,4}\$force_anon = ([^\r\n;]+);', source, re.M)
    expected = [
        "( ( FORCED_ANON || META_BOARD ) && $lr_data['capcode'] != 'admin' && $lr_data['capcode'] != 'admin_highlight' )",
        "( ( FORCED_ANON || META_BOARD ) && $res['capcode'] != 'admin' && $res['capcode'] != 'admin_highlight' )",
    ]
    if [value.decode('ascii') for value in expressions] != expected:
        raise RuntimeError('Catalog predicates differ from the audited expressions.')
    # Use local booleans in place of constants; every operator and comparison
    # remains source-derived. No application, includes, sessions or SQL execute.
    predicates = [value.replace('FORCED_ANON', '$forced').replace('META_BOARD', '$meta') for value in expected]
    program = '''$predicates=json_decode(stream_get_contents(STDIN),true,flags:JSON_THROW_ON_ERROR);
    $cases=[];
    foreach ([false,true] as $forced) foreach ([false,true] as $meta)
    foreach (['none','mod','admin','admin_highlight','manager','developer','founder','unknown'] as $badge)
    foreach ($predicates as $index=>$predicate) {
        $res=$lr_data=['capcode'=>$badge];
        $suppressed=eval('return '.$predicate.';');
        $cases[]=['forced_anonymous'=>$forced,'meta_board'=>$meta,'capcode'=>$badge,
            'context'=>$index===0?'last_reply':'op','identity_visible'=>!$suppressed];
    }
    echo json_encode(['php'=>PHP_VERSION,'cases'=>$cases],flags:JSON_THROW_ON_ERROR);'''
    result = subprocess.run(['php', '-d', 'memory_limit=16M', '-r', program],
                            input=json.dumps(predicates).encode(), capture_output=True,
                            timeout=5, check=True)
    if result.stderr or len(result.stdout) > 16_384:
        raise RuntimeError('Catalog predicate output exceeds its bounds.')
    output = json.loads(result.stdout)
    if len(output['cases']) != 64:
        raise RuntimeError('Catalog predicate case count differs.')
    fixture = {'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
               'files': {'catalog.php': digest}, 'php': output['php'], 'cases': output['cases']}
    encoded = (json.dumps(fixture, indent=2, ensure_ascii=True) + '\n').encode()
    if len(encoded) > 32_768:
        raise RuntimeError('Catalog predicate fixture exceeds its bounds.')
    if args.check:
        if args.output.read_bytes() != encoded:
            raise RuntimeError('Catalog predicate reference differs.')
    else:
        args.output.write_bytes(encoded)
    print(json.dumps({'catalog_identity_cases': len(output['cases'])}))


if __name__ == '__main__':
    main()
