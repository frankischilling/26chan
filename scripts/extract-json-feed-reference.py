#!/usr/bin/env python3
"""Execute source JSON feed generators over a controlled, ordered cache.

SQL, cache population, identity, formatting and writes are helper boundaries.
The PHP 5 each() helper is supplied for PHP 8; no source function is rewritten.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parents[1]
IMGBOARD_PIN='caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
PIN='18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868'
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('source',type=Path,nargs='?')
p.add_argument('--php',default='php')
p.add_argument('--check',action='store_true')
a=p.parse_args()
path=ROOT/'fixtures/json-feed-reference.json'
if a.source:
    raw=(a.source/'json.php').read_bytes()
    assert hashlib.sha256(raw).hexdigest()==PIN
    text=raw.decode().replace('\r\n','\n')
    block=text[text.index('function generate_thread_json('):].strip()
    raw=(a.source/'imgboard.php').read_bytes()
    assert hashlib.sha256(raw).hexdigest()==IMGBOARD_PIN
    text=raw.decode().replace('\r\n','\n')
    start=text.index('function get_json_tail_size($thread_id) {')
    tail=text[start:text.index('\n/**',start)].strip()
else:
    assert a.check
    saved=json.loads(path.read_text())
    assert saved['source_sha256']==PIN
    block=saved['generator_php']
    assert saved['imgboard_sha256']==IMGBOARD_PIN
    tail=saved['tail_generator_php']
assert hashlib.sha256(block.encode()).hexdigest()=='686cd71d326cf4da8d71a3ca76bba8e0281cde4f50484be52ea78ae0ca3eae7a'
assert hashlib.sha256(tail.encode()).hexdigest()=='a7faacbe8061777676d13d309a5862e7333397226cda52750c3a4882470ff7f0'
driver=r'''
$case=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
foreach (['DEF_PAGES'=>2,'PAGE_MAX'=>$case['page_max'],'REPLIES_SHOWN'=>$case['shown'],
 'INDEX_DIR'=>'/out/','RES_DIR'=>'/out/thread/','BOARD_DIR'=>'test',
 'JSON_TAIL_SIZE'=>2,'SHOW_THREAD_UNIQUES'=>true,'UPLOAD_BOARD'=>false,'SHOW_COUNTRY_FLAGS'=>false,
 'ENABLE_BOARD_FLAGS'=>false,'META_BOARD'=>false,'FORCED_ANON'=>false,
 'SPOILERS'=>true,'SPOILER_NUM'=>4,'STICKY_CAP'=>99] as $key=>$val) define($key,$val);
function each(&$array) { $key=key($array); if($key===null)return false; $value=current($array);next($array);return [1=>$value,'value'=>$value,0=>$key,'key'=>$key]; }
function log_cache($zero,$thread) {}
function auto_link($text,$thread){return $text;}
function get_unique_ip_count($thread){return 2;}
function print_page($path,$body,...$ignored){global $files;$files[$path]=json_decode($body,true,32,JSON_THROW_ON_ERROR);}
function mysql_board_call($query){global $case; if($query!=="SELECT no FROM `test` WHERE archived = 1 AND resto = 0 ORDER BY no ASC")throw new Exception('Unexpected SQL');return (object)['rows'=>$case['archived'],'at'=>0];}
function mysql_fetch_row($res){return isset($res->rows[$res->at])?[$res->rows[$res->at++]]:[null];}
$log=['THREADS'=>[]];$files=[];$index_rbl=100;$thread_unique_ips=0;
function row($id,$resto) {
 return ['no'=>(string)$id,'resto'=>(string)$resto,'now'=>'01/01/24(Mon)00:00:00','time'=>'1704067200',
 'name'=>'Anonymous','sub'=>'','com'=>'plain','host'=>'private-host','pwd'=>'private-password',
 '4pass_id'=>'private-account','email'=>'private-email','permasage'=>0,
 'tim'=>0,'filename'=>'','ext'=>'','w'=>0,'h'=>0,'tn_w'=>0,'tn_h'=>0,
 'md5'=>'','fsize'=>0,'filedeleted'=>0,'m_img'=>0,'since4pass'=>0,
 'id'=>'','capcode'=>'none','country'=>'','undead'=>0,'permaage'=>0,'sticky'=>0,'closed'=>0,
 'archived'=>0,'root'=>'','last_modified'=>(string)(1704067200+$id)];
}
foreach ($case['threads'] as $thread) {
 $id=$thread['id'];$log['THREADS'][]=$id;$op=row($id,0);
 $op['sticky']=$thread['sticky'];$op['undead']=$thread['undead'] ?? false;$op['children']=[];$op['semantic_url']='plain';
 $op['bumplimit']=false;$op['imagelimit']=false;$op['replycount']=count($thread['children']);$op['imgreplycount']=0;
 foreach($thread['children'] as $child) {
  $reply=row($child['id'],$id);
  if($child['image']) {
   $reply=array_replace($reply,['fsize'=>100,'filename'=>'sample','ext'=>'.jpg','tim'=>$child['id']*1000,
    'md5'=>'1234567890abcdef1234567890abcdef','w'=>640,'h'=>480,'tn_w'=>250,'tn_h'=>187]);
  }
  $reply['filedeleted']=$child['deleted'];
  if($child['image']&&!$child['deleted'])$op['imgreplycount']++;
  $op['children'][$child['id']]=true;$log[$child['id']]=$reply;
 }
 $log[$id]=$op;
}
''' + block + '\n' + tail + r'''
generate_index_json();generate_board_catalogue();generate_board_threads_json();generate_board_archived_json();
foreach($case['threads'] as $thread){generate_thread_json($thread['id']);$size=get_json_tail_size($thread['id']);if($size)generate_thread_json($thread['id'],false,false,false,$size);}
echo json_encode($files,JSON_THROW_ON_ERROR);
'''
cases=[]
for count in [1,2,3,5]:
 for shown in [0,1,3]:
  for maximum in [0,2]:
   threads=[]
   for index in range(count):
    tid=(count-index)*100
    # Deliberately nonnumeric insertion order; source sorts reply IDs naturally.
    children=[dict(id=tid+offset,image=offset%2==0,deleted=offset==4) for offset in [5,2,4,1,3]]
    if index==1: children=[]
    threads.append(dict(id=tid,sticky=index==0,children=children))
   case=dict(page_max=maximum,shown=shown,threads=threads,archived=[10,20,30])
   result=subprocess.run([a.php,'-d','memory_limit=64M','-d','max_execution_time=5','-r',driver],
       input=json.dumps(case).encode(),capture_output=True,check=True,timeout=10)
   assert not result.stderr,result.stderr.decode()
   case['expected']=json.loads(result.stdout)
   cases.append(case)
# Text-only controlled caches can be reproduced through actual SQL/HTTP tests
# without substituting unapproved attachment rows for real media receipts.
for original in list(cases):
    case=json.loads(json.dumps(original))
    case.pop('expected')
    case['text_fixture']=True
    for thread in case['threads']:
        for child in thread['children']:
            child['image']=False
    result=subprocess.run([a.php,'-d','memory_limit=64M','-d','max_execution_time=5','-r',driver],
        input=json.dumps(case).encode(),capture_output=True,check=True,timeout=10)
    assert not result.stderr,result.stderr.decode()
    case['expected']=json.loads(result.stdout)
    cases.append(case)
for sticky in [False,True]:
 for undead in [False,True]:
  for count in [0,3,4,5,7,8,9]:
   case=dict(page_max=0,shown=1,text_fixture=True,archived=[10,20,30],
       threads=[dict(id=100,sticky=sticky,undead=undead,
           children=[dict(id=100+i,image=False,deleted=False) for i in range(count,0,-1)])])
   result=subprocess.run([a.php,'-d','memory_limit=64M','-d','max_execution_time=5','-r',driver],
       input=json.dumps(case).encode(),capture_output=True,check=True,timeout=10)
   assert not result.stderr,result.stderr.decode()
   case['expected']=json.loads(result.stdout)
   cases.append(case)
fixture=dict(source_sha256=PIN,imgboard_sha256=IMGBOARD_PIN,scope=__doc__,generator_php=block,tail_generator_php=tail,cases=cases)
encoded=json.dumps(fixture,ensure_ascii=False,indent=2)+'\n'
if a.check: assert path.read_text()==encoded
else: path.write_text(encoded)
print(f'Executed all source JSON feed generators for {len(cases)} controlled caches.')
