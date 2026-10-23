#!/usr/bin/env python3
"""Replay the pinned upload-board tag table and numeric category admission."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PIN = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path, nargs='?')
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
path = ROOT / 'fixtures/upload-board-reference.json'
if args.source:
    raw = (args.source / 'imgboard.php').read_bytes()
    assert hashlib.sha256(raw).hexdigest() == PIN
    source = raw.decode().replace('\r\n', '\n')
    start = source.index('\t\tif( !$resto && !array_key_exists( (int)$filetag, $tags ) )')
    end = source.index('\n\t\t\n', start)
    admission = source[start:end].strip()
    table = (args.source / 'views/upboard.php').read_text().replace('\r\n', '\n')
    table = table[table.index('function upboard_tags()'):table.index('function updatelog(')].strip()
else:
    assert args.check
    saved = json.loads(path.read_text())
    admission, table = saved['admission_php'], saved['tags_php']
assert hashlib.sha256(table.encode()).hexdigest() == '6104ed7ff6b2b73f9096df5213b67b010a77dabf15599b06346ea8afb106107d'
assert hashlib.sha256(admission.encode()).hexdigest() == 'fbf43c10622f1baa397e138433659186bad758b72ecd936828544d0ef8ce4e26'
values = ['0','1','2','3','4','5','6','7','-1','-0','00','0006','+2',' 2','2 ', '2.9', '.9', '-.9', '6.99', '7.0', '1e0', '1e1','1e-999','1e999','NaN','INF','',' ','1x','0x1','1\x00','\t3\n','١','３','1_0']
driver = table + '''
function error($message,$field){throw new Exception($message);}
$cases=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
$out=[];$tags=upboard_tags();
foreach($cases as $filetag){$resto=0;$sub='owned subject';try{
''' + admission + '''
$out[]=['accepted'=>true,'subject'=>$sub];
}catch(Exception $e){$out[]=['accepted'=>false];}}
echo json_encode(['tags'=>$tags,'cases'=>$out],JSON_THROW_ON_ERROR);
'''
result = subprocess.run([args.php,'-d','memory_limit=16M','-d','max_execution_time=3','-r',driver], input=json.dumps(values).encode(),capture_output=True,check=True,timeout=5)
assert not result.stderr,result.stderr.decode()
output=json.loads(result.stdout)
fixture=dict(source_imgboard_sha256=PIN, tags_php=table, admission_php=admission,
             tags=output['tags'], cases=[dict(input=value,expected=expected) for value,expected in zip(values,output['cases'])])
encoded=json.dumps(fixture,ensure_ascii=False,indent=2)+'\n'
if args.check:
    assert path.read_text()==encoded
else:
    path.write_text(encoded)
print(f'Checked {len(values)} source category admission cases.')
