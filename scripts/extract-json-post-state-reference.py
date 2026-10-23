#!/usr/bin/env python3
"""Run pinned public post/tail serializers against source-computed thread states.

Helper boundaries: comments are already-formatted plain text, semantic context
is supplied, and identity/flag helpers are disabled. No banskip/private endpoint
is invoked. This corpus qualifies conditional fields, not those helpers.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PINS = {'json.php': '18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868',
        'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'}
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path, nargs='?')
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
path = ROOT / 'fixtures/json-post-state-reference.json'
if args.source:
    sources = {}
    for name, pin in PINS.items():
        raw = (args.source / name).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == pin
        sources[name] = raw.decode().replace('\r\n', '\n')
    source = sources['json.php']
    generator = source[source.index('function post_json_force_type('):source.index('function generate_index_json(')].strip()
    source = sources['imgboard.php']
    start = source.index('  else if ($is_single_thread) {')
    cache = source[start:source.index('\n\n\t// calculate old-status', start)].strip()
    # Preserve the complete branch body, changing only its entry condition.
    cache = cache.removeprefix('else if ($is_single_thread) {')[:-1]
else:
    assert args.check
    saved = json.loads(path.read_text())
    assert saved['source'] == PINS
    generator, cache = saved['generator_php'], saved['cache_php']

# Exact slices are independently pinned, including fixture-only CI replay.
assert hashlib.sha256(generator.encode()).hexdigest() == 'e73f3370b0134f153e291d8dbc6bca30bc8d53d3614581695da9527b750821e2'
assert hashlib.sha256(cache.encode()).hexdigest() == 'b661fe3a60575cd2f2a94b98ee6fbbf323ca388690d1acc58ae86daedc557aee'
driver = r'''
$cases=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
define('MAX_RES', 5); define('MAX_IMGRES', 3); define('STICKY_CAP', 99);
define('UPLOAD_BOARD', false); define('SHOW_COUNTRY_FLAGS', false);
define('ENABLE_BOARD_FLAGS', false); define('META_BOARD', false);
define('FORCED_ANON', false); define('SPOILERS', true); define('SPOILER_NUM', 4);
function auto_link($text,$thread){return $text;}
function generate_href_context($sub,$com){global $case;return $case['context'];}
''' + generator + r'''
$output=[];
foreach ($cases as $case) {
 $thread=100;
 $row=['no'=>100,'resto'=>0,'now'=>'01/01/24(Mon)00:00:00','time'=>1704067200,
 'name'=>'Anonymous','sub'=>'','com'=>'','host'=>'private-host','pwd'=>'private-password',
 '4pass_id'=>'private-account','email'=>'private-email','permasage'=>0,
 'tim'=>0,'filename'=>'','ext'=>'','w'=>0,'h'=>0,'tn_w'=>0,'tn_h'=>0,
 'md5'=>'','fsize'=>0,'filedeleted'=>0,'m_img'=>0,'since4pass'=>0,
 'id'=>'','capcode'=>'none','country'=>'','undead'=>$case['undead'],
 'permaage'=>$case['permaage'],'sticky'=>$case['sticky'],'closed'=>$case['closed'],
 'archived'=>0,'root'=>'2024-01-01 00:00:00','last_modified'=>1704067200,
 'replycount'=>$case['replies'],'imgreplycount'=>$case['images']];
 $row=array_replace($row,$case['media'] ?? []);
 $log=[$thread=>$row];
''' + cache + r'''
 $extra=['replies'=>$case['replies'],'images'=>$case['images']];
 $full=generate_post_json($log[$thread],$thread,$extra,false);
 $tail=generate_op_tail_json($log[$thread],$extra);
 foreach (['host','pwd','4pass_id','email','root','last_modified','replycount','imgreplycount'] as $key) {
   if (array_key_exists($key,$full)) throw new Exception('Private/cache field escaped');
 }
 $output[]=['full'=>$full,'tail'=>$tail];
}
echo json_encode($output,JSON_THROW_ON_ERROR);
'''
cases=[]
for sticky, permaage, undead, closed, replies, images, context in itertools.product(
        [False, True], [False, True], [False, True], [False, True], [4, 5, 6], [2, 3, 4], ['', 'plain-context']):
    cases.append(dict(sticky=sticky,permaage=permaage,undead=undead,closed=closed,
                      replies=replies,images=images,context=context))
state_count=len(cases)
for deleted, mobile, has_file, year in itertools.product([0,1], [0,1], [False,True], [0,2024,10000]):
    cases.append(dict(sticky=False,permaage=False,undead=False,closed=False,
        replies=0,images=0,context='',media=dict(filedeleted=deleted,m_img=mobile,
        ext='.jpg' if has_file else '',filename='sample',tim=1704067200123,
        w=640,h=480,tn_w=250,tn_h=187,fsize=100,md5='1234567890abcdef1234567890abcdef',since4pass=year)))
result = subprocess.run([args.php,'-d','memory_limit=64M','-d','max_execution_time=10','-r',driver],
    input=json.dumps(cases).encode(),capture_output=True,check=True,timeout=15)
assert not result.stderr, result.stderr.decode()
outputs=json.loads(result.stdout)
assert len(outputs)==len(cases)
for case, expected in zip(cases,outputs):
    case['expected']=expected
upload_cases=[]
for code in ['0','1','2','3','4','5','6','00','0006']:
    for title in ['', 'plain title', 'second title']:
        upload_cases.append(dict(sticky=False,permaage=False,undead=False,closed=False,
            replies=0,images=0,context='',media=dict(sub=code+'|'+title)))
result=subprocess.run([args.php,'-d','memory_limit=64M','-d','max_execution_time=10','-r',
    driver.replace("define('UPLOAD_BOARD', false)", "define('UPLOAD_BOARD', true)")],
    input=json.dumps(upload_cases).encode(),capture_output=True,check=True,timeout=15)
assert not result.stderr,result.stderr.decode()
for case,expected in zip(upload_cases,json.loads(result.stdout)):
    case['expected']=expected
swf_cases=[]
for deleted in [0,1]:
    swf_cases.append(dict(sticky=False,permaage=False,undead=False,closed=False,
        replies=0,images=0,context='',media=dict(sub='4|Owned movie',filedeleted=deleted,
        ext='.swf',filename='owned movie',tim=1704067200123,w=640,h=480,tn_w=0,tn_h=0,
        fsize=188,md5='000102030405060708090a0b0c0d0e0f',m_img=0)))
result=subprocess.run([args.php,'-d','memory_limit=64M','-d','max_execution_time=10','-r',
    driver.replace("define('UPLOAD_BOARD', false)", "define('UPLOAD_BOARD', true)")],
    input=json.dumps(swf_cases).encode(),capture_output=True,check=True,timeout=15)
assert not result.stderr,result.stderr.decode()
for case,expected in zip(swf_cases,json.loads(result.stdout)):
    case['expected']=expected
fixture={'source':PINS,'scope':__doc__,'generator_php':generator,'cache_php':cache,'cases':cases[:state_count],'media_cases':cases[state_count:],'upload_cases':upload_cases,'swf_cases':swf_cases}
encoded=json.dumps(fixture,ensure_ascii=False,indent=2)+'\n'
if args.check:
    assert path.read_text()==encoded
else:
    path.write_text(encoded)
print(f'Executed {len(cases)} source post/tail states and {len(upload_cases)} upload-board subjects, plus {len(swf_cases)} SWF file states.')
