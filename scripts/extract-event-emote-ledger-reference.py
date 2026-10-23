#!/usr/bin/env python3
"""Replay dormant emote buys/rolls with an owned account and seeded source draws.

Database, session authentication and the closed entry point are outside this
oracle. Only allowlisted in-memory reads/writes can occur. Seeds record source
outcomes; production randomness need not reproduce PHP's generator.
"""
import argparse,hashlib,json,re,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
PIN='8322e1123561001ab624da00932ebe94e2779839a1ae5d15c9c8413408310138'
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('source',type=Path,nargs='?');parser.add_argument('--php',default='php');parser.add_argument('--check',action='store_true');args=parser.parse_args()
path=ROOT/'fixtures/event-emote-ledger-reference.json'
roster=json.loads((ROOT/'fixtures/event-emote-reference.json').read_text())
if args.source:
 raw=(args.source/'emotes_xa22.php').read_bytes();assert hashlib.sha256(raw).hexdigest()==PIN
 source=raw.decode().replace('\r\n','\n')
 constants='\n'.join(line for line in source.splitlines() if re.match(r"define\('(ERR_|XA_ROLL_PRICE|XA_TICK_POINTS|XA_TICK_INTERVAL)",line))
 output=source[source.index('function xa_error('):source.index('/**\n * Sessions')].strip()
 roll=source[source.index('function xa_roll('):source.index('function xa_get_full_balance(')].strip()
 buy=source[source.index('function xa_buy('):source.index('// --------------')].strip()
 functions=constants+'\n\n'+output+'\n\n'+roll+'\n\n'+buy
else:
 assert args.check;saved=json.loads(path.read_text());assert saved['source_sha256']==PIN;functions=saved['functions']
FUNCTION_PIN='63925947d00b83b00efe57e6c6ae6da8ae70ddc119702b9eb94111faf5af50a1'
assert hashlib.sha256(functions.encode()).hexdigest()==FUNCTION_PIN
assert 'HMAC_SECRET' not in functions
mock=r'''
error_reporting(E_ALL);set_error_handler(function($severity,$message){throw new Exception($message);});
$case=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
$_COOKIE=['xa_sid'=>'owned-account'];$_POST=['eid'=>$case['id']??''];$_SERVER=['REQUEST_TIME'=>$case['now']];
$stored=$case['account'];$writes=0;$transactions=0;$commits=0;mt_srand($case['seed']);
function mysql_fetch_assoc($res){return ['data'=>$res];}
function mysql_global_call($query,...$args){
 global $stored,$writes,$transactions,$commits;
 $q=preg_replace('/\s+/',' ',trim($query));
 if($q==='START TRANSACTION'){$transactions++;return true;}
 if($q==='COMMIT'){$commits++;return true;}
 if($q==="SELECT data FROM april_emotes WHERE session_id = '%s' FOR UPDATE"){
  if($args[0]!=='owned-account')throw new Exception('Unknown owned session');
  return json_encode($stored,JSON_THROW_ON_ERROR);
 }
 if($q==="UPDATE april_emotes SET data = '%s' WHERE session_id = '%s' LIMIT 1"){
  if($args[1]!=='owned-account')throw new Exception('Unknown owned session');
  $stored=json_decode($args[0],true,32,JSON_THROW_ON_ERROR);$writes++;return true;
 }
 throw new Exception('Unrecognized owned SQL: '.$q);
}
ob_start();register_shutdown_function(function(){global $stored,$writes,$transactions,$commits;
 $text=ob_get_clean();echo json_encode(['response'=>json_decode($text,true,32,JSON_THROW_ON_ERROR),'account'=>$stored,'writes'=>$writes,'transactions'=>$transactions,'commits'=>$commits],JSON_THROW_ON_ERROR);
});
'''
cases=[]
for name,meta in roster['emotes'].items():
 id=name if meta[0]==1 else meta[1]
 for owned in [[],[name]]:
  cases.append({'action':'buy','id':id,'now':1000,'seed':1,'account':{'start_ts':1000,'balance':300,'owned':owned}})
for id in ['happy','03d18964','c458ef22','AngryWojak','unknown','../x','']:
 for balance,now in [(0,1000),(0,1300),(-30,1600)]:
  cases.append({'action':'buy','id':id,'now':now,'seed':1,'account':{'start_ts':1000,'balance':balance,'owned':[]}})
for size in [3,10]:
 for seed in [1,2,3,42,100]:
  for balance,now,owned in [(100,1000,[]),(0,1000,[]),(0,2500,[]),(100,1000,list(roster['emotes']))]:
   cases.append({'action':'roll','size':size,'now':now,'seed':seed,'account':{'start_ts':1000,'balance':balance,'owned':owned}})
for case in cases:
 code=mock+roster['metadata']+'\n'+roster['functions']+'\n'+functions+('\nxa_buy();' if case['action']=='buy' else '\nxa_roll('+str(case['size'])+');')
 result=subprocess.run([args.php,'-d','memory_limit=32M','-d','max_execution_time=5','-r',code],input=json.dumps(case).encode(),capture_output=True,check=False,timeout=10)
 assert result.returncode==0,(case,result.stderr.decode(),result.stdout.decode());assert not result.stderr,result.stderr.decode()
 case['expected']=json.loads(result.stdout)
encoded=json.dumps({'source_sha256':PIN,'scope':__doc__,'functions':functions,'cases':cases},ensure_ascii=False,indent=2)+'\n'
if args.check:assert path.read_text()==encoded
else:path.write_text(encoded)
print(f'Executed {len(cases)} dormant emote buy/roll cases against an owned ledger.')
