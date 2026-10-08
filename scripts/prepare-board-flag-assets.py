#!/usr/bin/env python3
"""Verify collected source UI bytes and scope their finite flag sprite rules."""
import argparse
import hashlib
import json
from pathlib import Path
import re

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('collected',type=Path)
parser.add_argument('--check',action='store_true')
args=parser.parse_args()
record_path=Path('docs/source-board-flag-assets.json')
assets=json.loads(record_path.read_text())
reference=json.loads(Path('apps/public/tests/fixtures/board-flags.json').read_text())
target=Path('apps/public/static/flags')
outputs=[]
scoped='/* Fixed source sprite rules, scoped to prevent cross-board code collisions. */\n'
for kind,version in [('pol',2),('mlp',3),('lgbt',1)]:
    for row in assets[kind]:
        source=args.collected/(kind+('.css' if row['url'].endswith('.css') else '.png'))
        raw=source.read_bytes()
        assert len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==row['sha256'],source
    text=(args.collected/(kind+'.css')).read_text()
    rules={m[1]:dict((key.strip(),value.strip()) for key,value in re.findall(r'([a-z-]+)\s*:\s*([^;}]+)',m[2]))
        for m in re.finditer(r'(\.bfl(?:-[a-z0-9]+)?)\s*\{([^}]+)\}',text)}
    assert set(rules)=={'.bfl'}|{'.bfl-'+code.lower() for code in reference['tables'][kind]['display']}
    geometry={}
    for selector,props in rules.items():
        if selector=='.bfl': continue
        merged=rules['.bfl']|props
        position=['0px' if value=='0' else value for value in merged['background-position'].split()]
        assert len(position)==2 and all(re.fullmatch(r'-?\d+px',value) for value in position)
        geometry[selector[5:]]={key:merged[key] for key in ['width','height']}
        geometry[selector[5:]]['position']=' '.join(position)
    assets[kind][0]['geometry']=geometry
    if kind!='pol':
        text=re.sub(r'\.bfl(-[a-z0-9]+)?',lambda m: '.bfl.bfl-type-'+kind if not m[1] else '.bfl-type-'+kind+'.bfl'+m[1],text)
        text=re.sub(r'url\([^)]*\)',f"url('/static/flags/{kind}-flags.{version}.png')",text)
        scoped+=text+'\n'
    name='board-flags.2.png' if kind=='pol' else f'{kind}-flags.{version}.png'
    assets[kind][1]['release_path']='/static/flags/'+name
    outputs.append((target/name,(args.collected/(kind+'.png')).read_bytes()))
scoped+='.bfl.bfl-type-test { background-image: none; }\n'
outputs += [(target/'board-types.css',scoped.encode()),(record_path,(json.dumps(assets,indent=2)+'\n').encode())]
for path,content in outputs:
    if args.check: assert path.read_bytes()==content,path
    else: path.write_bytes(content)
print('Verified three source sprite sets and 163 coordinates; test artwork remains unavailable.')
