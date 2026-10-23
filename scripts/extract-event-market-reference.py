#!/usr/bin/env python3
"""Exercise dormant stock-controller arithmetic against an owned in-memory ledger.

This does not bypass the live source's closed-market gate. Only the listed
functions are evaluated, with a synthetic verified account and fixed quotes.
No source bootstrap, credentials, network, database or real balances are used.
"""
import argparse, hashlib, json, subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
PIN='527f29c568739fc2a0340d8540f71016b5c02cd0712bc1fa070a27688f018be5'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('source',type=Path,nargs='?');parser.add_argument('--php',default='php');parser.add_argument('--check',action='store_true')
args=parser.parse_args();path=ROOT/'fixtures/event-market-reference.json'
if args.source:
 raw=(args.source/'xa24tb.php').read_bytes();assert hashlib.sha256(raw).hexdigest()==PIN
 source=raw.decode().replace('\r\n','\n')
 constants=source[source.index("define('CURRENCY_CODE'"):source.index('function output_json(')].strip()
 output=source[source.index('function output_json('):source.index('function create_account(')].strip()
 controllers=source[source.index('function get_account_balance('):source.index("output_error('The market is closed');")].strip()
 functions=constants+'\n\n'+output+'\n\n'+controllers
else:
 assert args.check
 saved=json.loads(path.read_text());assert saved['source_sha256']==PIN;functions=saved['functions']
FUNCTION_PIN='52227e065ae742a45c517de4611ad3480bf5031bf76372b01be0b222b5f609ca'
assert hashlib.sha256(functions.encode()).hexdigest()==FUNCTION_PIN
mock=r'''
error_reporting(E_ALL);
set_error_handler(function($severity,$message){if($severity===E_WARNING && (str_starts_with($message,'Undefined array key ')||in_array($message,['Trying to access array offset on value of type bool','Trying to access array offset on false'],true))) return true;throw new Exception($message);});
$case=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
$_POST=$case['fields'];$_COOKIE=['4chan_pass'=>'owned-synthetic'];$_SERVER=['REMOTE_ADDR'=>'192.0.2.1'];
$balance=$case['balance'];$holding=$case['holding'];$last=$case['last'];$writes=0;
class UserPwd {function __construct(...$args){} function isNew(){return false;} function getPwd(){return 'owned-account';}}
function rows($items){return (object)['rows'=>$items,'offset'=>0];}
function mysql_fetch_assoc($res){return $res->rows[$res->offset++]??false;}
function mysql_fetch_row($res){$row=mysql_fetch_assoc($res);return $row===false?false:array_values($row);}
function mysql_num_rows($res){return count($res->rows);}
function mysql_global_call($query,...$args){
 global $case,$balance,$holding,$last,$writes;
 $q=preg_replace('/\s+/',' ',trim($query));
 if(str_starts_with($q,'SELECT stock, SUM(amount) as amount FROM april_stock_users')){
  return rows([['stock'=>'_','amount'=>$balance],['stock'=>$case['fields']['stock'],'amount'=>$holding]]);
 }
 if(str_starts_with($q,'SELECT price FROM april_stock_prices')){
  return rows(in_array($args[0],STOCK_LIST,true)&&$case['current_price']>0?[['price'=>$case['current_price']]]:[]);
 }
 if(str_starts_with($q,'SELECT id FROM april_stock_users')) return rows($last>$case['now']-10?[['id'=>1]]:[]);
 if(preg_match("/^INSERT INTO april_stock_users \\(user_id, stock, amount\\) VALUES \\('%s', '([^']+)', (-?[0-9]+)\\)$/",$q,$match)){
  $stock=$match[1]==='%s'?$args[1]:$match[1];$amount=(int)$match[2];
  if($stock==='_')$balance+=$amount;elseif($stock===$case['fields']['stock'])$holding+=$amount;else throw new Exception('Unknown owned stock');
  $last=$case['now'];$writes++;return true;
 }
 throw new Exception('Unrecognized owned SQL: '.$q);
}
ob_start();register_shutdown_function(function(){global $balance,$holding,$last,$writes;
 $text=ob_get_clean();echo json_encode(['response'=>json_decode($text,true,32,JSON_THROW_ON_ERROR),'balance'=>$balance,'holding'=>$holding,'last'=>$last,'writes'=>$writes],JSON_THROW_ON_ERROR);
});
'''
base={'fields':{'stock':'PEPE','amount':'3','price':'10'},'balance':1000,'holding':20,'last':80,'now':100,'current_price':10}
cases=[]
def add(**updates):
 for side in ['buy','sell']:
  item={**base,**updates,'side':side};item['fields']={**base['fields'],**updates.get('fields',{})};cases.append(item)
for stock in ['PEPE','WOJK','ANIME','CHAD','CLOWN','LOL','SICP','AUTSM','BANE','CIA','BOOB','RDDT','DESU','JANNY','GME','CHUCK','YTSB','GACHI']:
 add(fields={'stock':stock})
for stock in ['', '_','0','pepe','UNKNOWN']:
 add(fields={'stock':stock})
for amount in ['', '0','-1','1001','1000','2.9','1e3','3suffix',' 3 ','abc']:
 add(fields={'amount':amount})
for price in ['', '0','-1','11','10suffix','1e1',' 10 ']:
 add(fields={'price':price})
for now in [79,80,89,90,91]:add(now=now)
for balance in [0,29,30]:add(balance=balance)
for holding in [0,2,3]:add(holding=holding)
for price in [0,-1,11]:add(current_price=price)
for case in cases:
 code=mock+functions+('\nbuy_stock();' if case['side']=='buy' else '\nsell_stock();')
 result=subprocess.run([args.php,'-d','memory_limit=32M','-d','max_execution_time=5','-r',code],input=json.dumps(case).encode(),capture_output=True,check=False,timeout=10)
 assert result.returncode==0,(case,result.stderr.decode(),result.stdout.decode())
 assert not result.stderr,result.stderr.decode();case['expected']=json.loads(result.stdout)
encoded=json.dumps({'source_sha256':PIN,'scope':__doc__,'functions':functions,'cases':cases},ensure_ascii=False,indent=2)+'\n'
if args.check:assert path.read_text()==encoded
else:path.write_text(encoded)
print(f'Executed {len(cases)} dormant source market cases on an owned ledger.')
