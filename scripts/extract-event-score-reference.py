#!/usr/bin/env python3
"""Run isolated dormant like-controller cases on an owned ledger, without source bootstrap."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
PIN = 'f62362649585ec9956cf412c8de9174ebdf6cac6c9b46f0d1a41bc4a2f835992'
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('source', type=Path, nargs='?')
p.add_argument('--php', default='php')
p.add_argument('--check', action='store_true')
a = p.parse_args()
path = ROOT / 'fixtures/event-score-reference.json'
if a.source:
    raw = (a.source / 'lib/like_score.php').read_bytes()
    assert hashlib.sha256(raw).hexdigest() == PIN
    text = raw.decode().replace('\r\n', '\n')
    functions = text[text.index("define('LIKE_MAX_LIKES'"):text.index('function like_is_ip_suspicious(')].strip()
    functions += '\n\n' + text[text.index('function like_is_ip_known('):text.index('function like_build_perk_html(')].strip()
else:
    assert a.check
    saved = json.loads(path.read_text())
    assert saved['source_sha256'] == PIN
    functions = saved['functions']
# The pinned isolated functions include no bootstrap, secrets or live DB access.
FUNCTION_PIN = '873f3371dba4fcd1c2dce94994adb0659cadf2a5ec0573eba23b8b0262f9a459'
assert hashlib.sha256(functions.encode()).hexdigest() == FUNCTION_PIN
mock = r'''
error_reporting(E_ALL);
set_error_handler(function($severity,$message){throw new Exception($message);});
$c=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
define('BOARD_DIR',$c['board']);
$_POST=$c['fields'];$_COOKIE=$c['cookie']===null?[]:['xa19'=>$c['cookie']];
if($c['auser'])$_COOKIE['4chan_auser']='';
$_SERVER=['REMOTE_ADDR'=>'192.0.2.1'];
$passid=$c['pass']?'owned-pass':false;$captcha_bypass=$c['bypass'];
$likes=$c['likes'];$scores=['192.0.2.1'=>100,'owned-pass'=>100,'owned-target'=>200];$logs=[];$captcha=0;
function rows($r){return (object)['rows'=>$r,'offset'=>0];}
function mysql_fetch_assoc($r){return $r->rows[$r->offset++]??false;}
function mysql_fetch_row($r){$v=mysql_fetch_assoc($r);return $v===false?false:array_values($v);}
function mysql_num_rows($r){return count($r->rows);}
function start_auth_captcha(){global $captcha;$captcha++;}
function end_recaptcha_verify(){global $c;if(!$c['captcha_ok'])die("0\nOwned CAPTCHA refused.");}
function isIPRangeBanned($ip){global $c;return $c['range_banned'];}
function log_cache(...$args){}
function updatelog(...$args){}
function mysql_board_call($query,...$args){
 global $c,$likes;
 $q=preg_replace('/\s+/',' ',trim($query));
 if(str_starts_with($q,'SELECT host, 4pass_id')) return rows($c['target']?[['host'=>$c['self_peer']?'192.0.2.1':'192.0.2.2','4pass_id'=>$c['self_pass']?'owned-pass':'owned-target']]:[]);
 if(str_starts_with($q,'SELECT email, resto, archived')) return rows($c['disappeared']?[]:[['email'=>'0.'.$likes,'resto'=>$c['op']?0:10,'archived'=>$c['archived']]]);
 if(preg_match("/^UPDATE `[a-z0-9]+` SET email = '0\\.([0-9]+)'/",$q,$m)){$likes=(int)$m[1];return true;}
 throw new Exception('Unexpected owned board SQL: '.$q);
}
function mysql_global_call($query,...$args){
 global $c,$scores,$logs;
 $q=preg_replace('/\s+/',' ',trim($query));
 if(str_starts_with($q,'SELECT id FROM like_user_log')){
  if(str_contains($q,'created_on >'))return rows($c['last']>$c['now']-120?[['id'=>1]]:[]);
  if(str_contains($q,'AND board ='))return rows($c['duplicate']?[['id'=>1]]:[]);
  return rows($c['known']?[['id'=>1]]:[]);
 }
 if(str_starts_with($q,'INSERT INTO `like_user_log`')){$logs[]=[$args[0],$args[1]];return true;}
 if(preg_match("/^INSERT INTO `like_user_scores` \\(`user_id`, `user_score`\\) VALUES \\('%s', ([0-9]+)\\)/",$q,$m)){$scores[$args[0]]+=$m[1];return true;}
 throw new Exception('Unexpected owned global SQL: '.$q);
}
ob_start();register_shutdown_function(function(){global $likes,$scores,$logs,$captcha;
 $response=ob_get_clean();echo json_encode(['response'=>$response,'likes'=>$likes,'scores'=>$scores,'log_count'=>count($logs),'captcha_calls'=>$captcha],JSON_THROW_ON_ERROR);
});
'''
base = dict(board='g', fields={'post_id':'100'}, cookie='100', pass_=False, bypass=False,
            captcha_ok=True, known=True, auser=False, range_banned=False, target=True, self_peer=False,
            self_pass=False, disappeared=False, op=False, archived=False, duplicate=False,
            likes=0, last=0, now=1000)
base['pass'] = base.pop('pass_')
cases = []
def add(**updates):
    cases.append({**base, **updates})
add()
for board in ['b','qa','s4s','bant','vip','a','pol','test']: add(board=board)
for fields,cookie in [({},None),({},'100'),({'post_id':'100'},None),({'post_id':'0'},'0'),({'post_id':'abc'},'abc'),({'post_id':'100'},'101'),({'post_id':'00100'},'100'),({'post_id':'00100'},'00100'),({'post_id':'100tail'},'100tail')]: add(fields=fields,cookie=cookie)
for key in ['target','self_peer','self_pass','disappeared','op','archived','duplicate','known','range_banned']:
    add(**{key:not base[key]})
for last in [879,880,881,1000,1001]: add(last=last)
for likes in [1,1048574,1048575,1048576]: add(likes=likes)
for pass_ in [False,True]:
    for bypass in [False,True]:
        for known in [False,True]:
            for captcha_ok in [False,True]: add(**{'pass':pass_,'bypass':bypass,'known':known,'captcha_ok':captcha_ok})
for known in [False,True]:
    for captcha_ok in [False,True]: add(auser=True,known=known,captcha_ok=captcha_ok)
for self_pass in [False,True]:
    for bypass in [False,True]: add(**{'pass':True,'bypass':bypass,'self_pass':self_pass,'range_banned':True})
for target in [False,True]:
    for duplicate in [False,True]: add(target=target,duplicate=duplicate,last=999,self_peer=True)
for board in ['g','b','qa','s4s','bant','vip']:
    for pass_ in [False,True]:
        for bypass in [False,True]: add(board=board,**{'pass':pass_,'bypass':bypass,'operation':'post'})
for case in cases:
    code=mock+functions+('\nlike_update_post_score();echo "posted";' if case.get('operation')=='post' else '\nlike_commit();')
    result=subprocess.run([a.php,'-d','memory_limit=32M','-d','max_execution_time=5','-r',code],input=json.dumps(case).encode(),capture_output=True,timeout=10)
    assert result.returncode==0 and not result.stderr,(case,result.stderr.decode(),result.stdout.decode())
    case['expected']=json.loads(result.stdout)
encoded=json.dumps({'source_sha256':PIN,'scope':__doc__,'functions':functions,'cases':cases},ensure_ascii=False,indent=2)+'\n'
if a.check: assert path.read_text()==encoded
else: path.write_text(encoded)
print('Function pin:',hashlib.sha256(functions.encode()).hexdigest())
print(f'Executed {len(cases)} dormant source scoring cases on an owned ledger.')
