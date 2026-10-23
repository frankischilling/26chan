#!/usr/bin/env python3
"""Execute the source directory builder with recorded rows and local RPC receipts.

Database rows are synthetic, not a claim about the unavailable source database.
The PHP 5 ctype helper returns false for non-string metadata; casting those
already-typed catalog-info values would not change them. No network or writes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parents[1]
PIN='caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('source',type=Path,nargs='?');p.add_argument('--php',default='php');p.add_argument('--check',action='store_true')
a=p.parse_args();path=ROOT/'fixtures/json-directory-build-reference.json'
if a.source:
 raw=(a.source/'imgboard.php').read_bytes();assert hashlib.sha256(raw).hexdigest()==PIN
 text=raw.decode().replace('\r\n','\n');start=text.index('function rebuild_boards_json()')
 block=text[start:text.index('\nfunction rebuild(',start)].strip()
else:
 assert a.check
 saved=json.loads(path.read_text());assert saved['source_sha256']==PIN
 block=saved['generator_php']
assert hashlib.sha256(block.encode()).hexdigest()=='a1394e7178fbfffced14bca5f347a77e9f4d922142a3804a1ab3aff16172ec60'
driver=r'''
namespace SourceDirectoryOracle;
$case=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
$captured=null;ob_start();
register_shutdown_function(function(){global $captured;ob_end_clean();echo json_encode($captured,JSON_THROW_ON_ERROR);});
define('BOARD_DIR','owned');define('BOARDS_ROOT','/out/');
function has_level(){global $case;return $case['authorized'];}
function fancystyle(){return '';}
function ctype_digit($value){return is_string($value)&&\ctype_digit($value);}
function mysql_global_call($sql){global $case;if($sql!=='SELECT dir as board,name as title FROM boardlist ORDER BY board ASC')throw new \Exception('Unexpected query');return (object)['at'=>0,'rows'=>$case['rows']];}
function mysql_fetch_assoc($query){return $query->rows[$query->at++] ?? false;}
function rpc_start_request($url,$post,$cookie,$internal){
 global $case;
 if($post!==['mode'=>'cataloginfo']||$cookie!==null||$internal!==true)throw new \Exception('Unexpected RPC authority');
 foreach($case['metadata'] as $board=>$data)if($url==="https://sys.int/$board/imgboard.php")return $board;
 throw new \Exception('Unrecognized local RPC receipt');
}
function rpc_finish_request($board,&$error,&$httperror){global $case;$error=null;$httperror=null;return json_encode($case['metadata'][$board],JSON_THROW_ON_ERROR);}
function print_page($path,$body){global $captured;if($path!=='/out/boards.json')throw new \Exception('Unexpected output');$captured=json_decode($body,true,32,JSON_THROW_ON_ERROR);}
''' + block + '\nrebuild_boards_json();'
metadata={c['board']:c['expected'] for c in json.loads((ROOT/'fixtures/json-directory-reference.json').read_text())['cases']}
cases=[]
for authorized in [False,True]:
 for description in ['plain description','123','000123','0','9223372036854775808','9'*100,'-1','1.0','１２３','<literal & description>']:
  case=dict(authorized=authorized,rows=[dict(board='a',title='Owned title'),dict(board='vp',title='Replaced source title')],
      metadata={'a':dict(metadata['a'],meta_description=description),'vp':metadata['vp']})
  result=subprocess.run([a.php,'-d','memory_limit=64M','-d','max_execution_time=5','-r',driver],input=json.dumps(case).encode(),capture_output=True,check=True,timeout=10)
  assert not result.stderr,result.stderr.decode()
  case['expected']=json.loads(result.stdout);cases.append(case)
fixture=dict(source_sha256=PIN,scope=__doc__,generator_php=block,cases=cases)
encoded=json.dumps(fixture,ensure_ascii=False,indent=2)+'\n'
if a.check:assert path.read_text()==encoded
else:path.write_text(encoded)
print(f'Executed {len(cases)} source directory build and authorization cases.')
