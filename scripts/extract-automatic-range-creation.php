<?php
// Selected moderation event logic only; no admin application or operator data.
if($argc<3){fwrite(STDERR,"Usage: php extract-automatic-range-creation.php SOURCE OUTPUT [--check]\n");exit(2);}
$source=file_get_contents($argv[1].'/admin.php');
$hash='4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb';
if(hash('sha256',$source)!==$hash)throw new RuntimeException('Audited source differs.');
foreach(['process_auto_rangeban','add_auto_rangeban_log'] as $name){
 if(preg_match_all('/^function '.preg_quote($name,'/').'\b.*?^}/ms',$source,$match)!==1 || strlen($match[0][0])>8192)throw new RuntimeException('Selected body differs.');
 eval($match[0][0]);
}
unset($source,$match);
define('BOARD_DIR','g');
class Cursor {public function __construct(public int $count){}}
function mysql_fetch_row($cursor){return [$cursor->count];}
function mysql_global_call($query,$board=null,$browser=null,$prefix=null){
 $now=$GLOBALS['input']['now'];
 if($query==="DELETE FROM event_log WHERE type = 'rangeban_hint' AND created_on < DATE_SUB(NOW(), INTERVAL 1 HOUR)"){
  $GLOBALS['events']=array_values(array_filter($GLOBALS['events'],fn($e)=>$e['kind']!=='hint'||$e['created_at']>=$now-3600));return true;
 }
 if(!preg_match('/thread_id = ([0-9]+)/',$query,$thread))throw new RuntimeException('Selected event query differs.');
 if(str_contains($query,"type = 'rangeban_hint'")&&str_contains($query,'COUNT(DISTINCT ip)'))$kind='hint';
 elseif(str_contains($query,"type = 'rangeban'")&&str_contains($query,'created_on > DATE_SUB(NOW(), INTERVAL 1 HOUR)'))$kind='range';
 else throw new RuntimeException('Unexpected query.');
 $prefix=substr($prefix,0,-1);
 $rows=array_values(array_filter($GLOBALS['events'],fn($e)=>$e['kind']===$kind&&$e['board']===$board&&$e['browser']===$browser
  &&$e['parent']===(int)$thread[1]&&str_starts_with($e['ip'],$prefix)&&($kind==='hint'||$e['created_at']>$now-3600)));
 return new Cursor($kind==='hint'?count(array_unique(array_column($rows,'ip'))):count($rows));
}
function write_to_event_log($kind,$ip,$fields){
 if(!in_array($kind,['rangeban','rangeban_hint'],true))throw new RuntimeException('Unexpected event type.');
 $GLOBALS['written'][]=['kind'=>$kind==='rangeban'?'range':'hint','ip'=>$ip,'fields'=>$fields];return true;
}
set_error_handler(function(){throw new RuntimeException('Unexpected selected-source warning.');});
$now=1700000000;$base=['now'=>$now,'ip'=>'192.0.2.10','browser'=>'1deadbeef','parent'=>123,'origin'=>0,'events'=>[]];
$event=['id'=>1,'kind'=>'hint','ip'=>'192.0.2.10','browser'=>'1deadbeef','parent'=>123,'board'=>'g','created_at'=>$now];
$cases=[];
function record($input){
 $GLOBALS['input']=$input;$GLOBALS['events']=$input['events'];$GLOBALS['written']=[];
 if(process_auto_rangeban($input['ip'],$input['browser'],$input['parent'],1234,7,$input['origin'])!==true || count($GLOBALS['written'])>1)
  throw new RuntimeException('Unexpected event plan.');
 $input['action']=$GLOBALS['written'][0]['kind']??'unchanged';
 $input['retained_ids']=array_column($GLOBALS['events'],'id');$GLOBALS['cases'][]=$input;
}
foreach([0,1] as $origin)foreach([null,-1,0,3599,3600,3601] as $hintAge)foreach([null,-1,0,3599,3600,3601] as $rangeAge){
 $events=[];
 if($hintAge!==null)$events[]=array_replace($event,['created_at'=>$now-$hintAge]);
 if($rangeAge!==null)$events[]=array_replace($event,['id'=>2,'kind'=>'range','created_at'=>$now-$rangeAge]);
 record(array_replace($base,['origin'=>$origin,'events'=>$events]));
}
foreach([0,1] as $origin)foreach([
 ['board'=>'a'],['parent'=>124],['ip'=>'192.1.2.10'],['browser'=>'1badc0ffe'],['ip'=>'192.0.9.99'],[]
] as $change)foreach(['hint','range'] as $kind)
 record(array_replace($base,['origin'=>$origin,'events'=>[array_replace($event,$change,['kind'=>$kind])]]));
$payload=['source_sha256'=>$hash,'scope'=>'Authorized non-warning global action and trusted target metadata are caller prerequisites. SQL callbacks model only the selected statements.','cases'=>[]];
$json=json_encode($payload,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR);
$rows=array_map(fn($case)=>'        '.json_encode($case,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),$cases);
$json=str_replace('"cases": []','"cases": ['."\n".implode(",\n",$rows)."\n    ]",$json)."\n";
if(count($cases)!==96||strlen($json)>128*1024)throw new RuntimeException('Unexpected corpus bounds.');
if(in_array('--check',$argv,true)){if(file_get_contents($argv[2])!==$json)throw new RuntimeException('Creation fixture differs.');}else file_put_contents($argv[2],$json);
printf("Automatic creation fixture matches %d cases.\n",count($cases));
