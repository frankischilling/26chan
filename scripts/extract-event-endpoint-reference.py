#!/usr/bin/env python3
"""Replay the source's unconditional closed-event gates without its includes.

Database/bootstrap includes are excluded. A sentinel rejects any attempt to
reach dormant account, purchase, roll or sale handlers.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PINS = {'emotes_xa22.php': '8322e1123561001ab624da00932ebe94e2779839a1ae5d15c9c8413408310138',
        'xa24tb.php': '527f29c568739fc2a0340d8540f71016b5c02cd0712bc1fa070a27688f018be5'}
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path, nargs='?')
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
path = ROOT / 'fixtures/event-endpoint-reference.json'
if args.source:
    sources = {}
    for name, pin in PINS.items():
        raw = (args.source / name).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == pin
        sources[name] = raw.decode().replace('\r\n', '\n')
    emotes = sources['emotes_xa22.php']
    market = sources['xa24tb.php']
    blocks = {'/emotes_xa22.php': emotes[len('<?php'):emotes.index('//define(')].strip(),
              '/xa24tb.php': market[market.index('function output_json('):market.index('function create_account(')].strip() + '\n\n' + market[market.index("output_error('The market is closed');"):].strip()}
else:
    assert args.check
    saved = json.loads(path.read_text())
    assert saved['source'] == PINS
    blocks = saved['blocks']
BLOCK_PINS = {'/emotes_xa22.php': '6214e78ae572fadb30619853af52062d407389408a663f81c5ea905110dd0246', '/xa24tb.php': '7de1d3609589805391c65f0e341fd801044eac6e41627361b66509e9586e53a7'}
assert {name: hashlib.sha256(value.encode()).hexdigest() for name,value in blocks.items()} == BLOCK_PINS
requests = [{}, {'action':'start'}, {'action':'roll3'}, {'action':'roll10'}, {'action':'buy','eid':'Owned'},
            {'create':'1'}, {'mode':'buy','stock':'PEPE','amount':'1','price':'1'},
            {'mode':'sell','stock':'PEPE','amount':'1','price':'1'}, {'action':['buy'],'mode':['sell']}]
driver = r'''
$_POST=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
$_GET=$_POST; $_COOKIE=['xa_sid'=>'owned-untrusted','4chan_pass'=>'owned-untrusted'];
function create_account(){throw new Exception('Dormant account reached');}
function buy_stock(){throw new Exception('Dormant purchase reached');}
function sell_stock(){throw new Exception('Dormant sale reached');}
ob_start(); http_response_code(200);
register_shutdown_function(function(){
 $body=ob_get_clean(); echo json_encode(['status'=>http_response_code(),'body'=>$body],JSON_THROW_ON_ERROR);
});
'''
cases = []
for route, block in blocks.items():
    for request in requests:
        result = subprocess.run([args.php,'-d','memory_limit=32M','-d','max_execution_time=5','-r',driver+block],
                                input=json.dumps(request).encode(),capture_output=True,check=True,timeout=10)
        assert not result.stderr, result.stderr.decode()
        cases.append({'route':route,'fields':request,'expected':json.loads(result.stdout)})
fixture = {'source':PINS,'scope':__doc__,'blocks':blocks,'cases':cases}
encoded = json.dumps(fixture,ensure_ascii=False,indent=2)+'\n'
if args.check:
    assert path.read_text()==encoded
else:
    path.write_text(encoded)
print(f'Executed {len(cases)} closed event action cases.')
