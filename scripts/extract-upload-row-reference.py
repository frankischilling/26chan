#!/usr/bin/env python3
"""Run the upload listing's exact label/size statements over owned escaped fields.

Fields are already sanitized and comments already formatted at this boundary.
This does not qualify the source database, sanitizer or comment formatter.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path,nargs='?');parser.add_argument('--php',default='php');parser.add_argument('--check',action='store_true')
args=parser.parse_args();path=ROOT/'fixtures/upload-row-reference.json'
if args.source:
    raw=(args.source/'views/upboard.php').read_bytes()
    assert hashlib.sha256(raw).hexdigest()=='5514c113e492d8aeb629e084198a3c9462bc0eca97685088c3c53b5dd96591c4'
    source=raw.decode().replace('\r\n','\n')
    start=source.index('\t\t\tif( $fsize >= 1048576 )',source.index('/** BUILD /f/ INDEX **/'))
    listing=source[start:source.index('\t\t\t\t$replink',start)].strip()
    start=source.index('\t\t\t\tif( $fsize >= 1048576 )')
    thread=source[start:source.index('\t\t\t\tif( $filedeleted )',start)].strip()
else:
    assert args.check
    saved=json.loads(path.read_text());listing,thread=saved['listing_php'],saved['thread_php']
assert hashlib.sha256(listing.encode()).hexdigest()=='745ff4192f1414bf909ad66bcf06b8d5967a7f9010ad089053e3c93eb0efad3b'
assert hashlib.sha256(thread.encode()).hexdigest()=='862ab0affe947a91c63e9675492bae5725f5c47eface0b561e670f7976b6835c'
labels=json.loads((ROOT/'fixtures/upload-board-reference.json').read_text())['tags_php']
cases=[]
for size,name,subject in itertools.product([0,1,1023,1024,1048,1572,1048575,1048576,1179648,10485760],
        ['owned','x'*25,'x'*26,'猫'*26,'x'*23+'&<"'],['','plain subject','x'*30,'x'*31,'<unsafe>& subject']):
    cases.append(dict(bytes=size,filename=name,subject=subject,comment='Fallback &amp; text<br>ignored'))
for comment in ['<span class="quote">&gt;quoted</span>','plain <strong>bold</strong>','x'*31,'literal &amp;#44; text']:
    cases.append(dict(bytes=1024,filename='owned',subject='',comment=comment))
driver=labels+'''
$cases=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);$out=[];$tags=upboard_tags();
foreach($cases as $case){$fsize=$case['bytes'];$filename=htmlspecialchars($case['filename'],ENT_QUOTES,'UTF-8');
$sub=htmlspecialchars($case['subject'],ENT_QUOTES,'UTF-8');$com=$case['comment'];$no=100;$delarr=[];$tag=4;
$shortname=$shortsub=null;
'''+listing+'''\n}\n'''+thread+'''
$out[]=['visible'=>$kbsize!=='0 B','filename'=>$shortname,'subject'=>$shortsub,'listing_size'=>$kbsize,'thread_size'=>$size.'B'];
}echo json_encode($out,JSON_THROW_ON_ERROR);
'''
result=subprocess.run([args.php,'-d','memory_limit=32M','-d','max_execution_time=5','-r',driver],input=json.dumps(cases).encode(),capture_output=True,check=True,timeout=10)
assert not result.stderr,result.stderr.decode()
for case,value in zip(cases,json.loads(result.stdout)):case['expected']=value
fixture=dict(scope=__doc__,listing_php=listing,thread_php=thread,listing_sha256=hashlib.sha256(listing.encode()).hexdigest(),thread_sha256=hashlib.sha256(thread.encode()).hexdigest(),cases=cases)
encoded=json.dumps(fixture,ensure_ascii=False,indent=2)+'\n'
if args.check:assert path.read_text()==encoded
else:path.write_text(encoded)
print(f'Checked {len(cases)} source upload-row cases.')
