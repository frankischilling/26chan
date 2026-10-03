"""Execute pinned spoiler preparation and JSON projection on synthetic subjects."""
import argparse
import hashlib
import itertools
import json
import re
import subprocess
from pathlib import Path

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path)
parser.add_argument('output',type=Path)
parser.add_argument('--check',action='store_true')
args=parser.parse_args()
hashes={
    'imgboard.php':'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'json.php':'18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868',
}
bodies={}
for name,pin in hashes.items():
    data=(args.source/name).read_bytes()
    if hashlib.sha256(data).hexdigest()!=pin:
        raise ValueError('Audited source changed: '+name)
    bodies[name]=data.decode().replace('\r\n','\n')
prepare=re.search(r'if\( SPOILERS == 1 && \$spoiler \) \{\s*\$sub = "SPOILER<>\$sub";\s*\}',bodies['imgboard.php']).group(0)
projection=bodies['json.php'][bodies['json.php'].index("$var['spoiler'] = 0;"):bodies['json.php'].index("if( $var['sub'] && !$var['resto'] && UPLOAD_BOARD )")].strip()
assert len(prepare)<200 and len(projection)<400
subjects=[('', ''),('Owned subject','Owned subject'),('SPOILER<>literal','SPOILER&lt;&gt;literal'),('雪 & tea','雪 &amp; tea')]
extract=re.search(r'extract\( \$_POST, EXTR_SKIP \);',bodies['imgboard.php']).group(0)
rows=[]
runtime=None
for enabled in [False,True]:
    recipes=[dict(enabled=enabled,raw_flag=raw_flag,attachment=attachment,
                  subject=raw,prepared_subject=prepared)
             for raw_flag,(raw,prepared),attachment in itertools.product([None,'','0','on','true','false','1','yes','00'],subjects,[False,True])]
    program=("$rows=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);"
             "define('SPOILERS',$rows[0]['enabled']);$results=[];foreach($rows as $row){"
             "unset($spoiler);$_POST=$row['raw_flag']===null?[]:['spoiler'=>$row['raw_flag']];"+extract+
             "$spoiler=$spoiler??null;$row['requested']=(bool)$spoiler;$sub=$row['prepared_subject'];"+prepare+
             "$stored=$sub;$var=['sub'=>$sub];"+projection+
             "$results[]=$row+['stored_subject'=>$stored,'json_subject'=>$var['sub'],"
             "'json_spoiler'=>$var['spoiler']??null];}"
             "echo json_encode(['php'=>PHP_VERSION,'rows'=>$results],JSON_THROW_ON_ERROR);")
    result=subprocess.run(['php','-d','memory_limit=64M','-d','max_execution_time=5','-r',program],
                          input=json.dumps(recipes).encode(),capture_output=True,timeout=10,check=True)
    assert not result.stderr and len(result.stdout)<65536
    value=json.loads(result.stdout)
    assert runtime is None or runtime==value['php']
    runtime=value['php']
    rows.extend(value['rows'])
fixture=dict(reference='operator-supplied 4chan-old checkout',source_revision='545b7812d1849f7958d914950c91fdbbe38f6b22',
             files=hashes,extractor_php=runtime,
             scope='POST scalar extraction, selected SPOILERS prefix preparation and JSON prefix decoding; attachment independence',
             boundary_stubs=['prepared escaped synthetic subjects','scalar checkbox choices and omitted field','synthetic attachment-presence bit'],
             excludes=['complete posting endpoint and formatter','database persistence','original page rendering','custom spoiler assets'],
             cases=rows)
encoded=json.dumps(fixture,ensure_ascii=False,indent=2)+'\n'
if args.check:
    assert args.output.read_text()==encoded,'Recorded source comparison changed.'
else:
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(encoded)
print(f'{len(rows)} public spoiler source cases verified with PHP {runtime}.')
