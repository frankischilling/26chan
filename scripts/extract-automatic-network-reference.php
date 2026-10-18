<?php
// Isolated hash-pinned decision bodies; no application, keys or live DB access.
if ($argc < 3) { fwrite(STDERR, "Usage: php extract-automatic-network-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$hashes=['lib/postfilter.php'=>'d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207',
 'lib/userpwd.php'=>'0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26'];
$sources=[];
foreach($hashes as $file=>$hash) {
 $sources[$file]=file_get_contents($argv[1].'/'.$file);
 if(hash('sha256',$sources[$file])!==$hash) throw new RuntimeException('Audited source differs.');
}
function selected($source,$names) {
 $out='';
 foreach($names as $name) {
  $pattern='/^([ \t]*)(?:public[ \t]+)?(function[ \t]+'.preg_quote($name,'/').'\b.*?^\1})[ \t]*\r?$/ms';
  if(preg_match_all($pattern,$source,$matches)!==1 || strlen($matches[2][0])>8192) throw new RuntimeException('Selected body differs.');
  $out.=$matches[2][0]."\n";
 }
 if(strlen($out)>32768) throw new RuntimeException('Selected bodies exceed bounds.');
 return $out;
}
$class='class UserPwd { public $now=0,$creation_ts=0,$mask_ts=0,$verified_level=0,$post_count=0,$report_count=0,$action_buffer=0,$ip_change_score=0; public function setCookie($domain) {}';
foreach(['IP_CHANGE_DELAY','IP_CHANGE_MASK_VAL','A_POST','A_REPORT'] as $name) {
 if(!preg_match('/const\s+'.$name.'\s*=\s*([0-9]+)\s*;/',$sources['lib/userpwd.php'],$match)) throw new RuntimeException('Missing constant.');
 $class.='const '.$name.'='.$match[1].';';
}
$methods=['pwdLifetime','maskLifetime','maskTs','verifiedLevel','isUserKnown','isUserKnownOrVerified','postCount','reportCount','ipChangeScore'];
eval($class.selected($sources['lib/userpwd.php'],$methods).'}');
eval(selected($sources['lib/postfilter.php'],['spam_filter_post_ip','is_ip_auto_rangebanned']));
unset($sources,$class);
define('CAPTCHA',false);define('BOARD_DIR','g');define('DEFAULT_BURICHAN',false);
foreach(['S_IPRANGE_BLOCKED','S_IPRANGE_BLOCKED_TEMP','S_IPRANGE_BLOCKED_L1','S_IPRANGE_BLOCKED_PASS'] as $constant) define($constant,'owned');
class L {public static function d($board){return 'example.invalid';}}
class OwnedBlocked extends RuntimeException {}
function error($message){throw new OwnedBlocked();}
function isIPRangeBanned($address,$asn,$options){return false;}
function spam_filter_get_browser_id(){return $GLOBALS['input']['browser'];}
function spam_filter_get_req_sig(){return 'owned-unused';}
function write_to_event_log($kind,$ip,$fields){if($kind!=='auto_range_hit')throw new RuntimeException('Unexpected event.');$GLOBALS['hits']++;}
class Cursor {public function __construct(public array $rows){}}
function mysql_global_call($query,$board,$browser,$prefix) {
 if(!str_contains($query,"type = 'rangeban'") || !str_contains($query,'created_on > DATE_SUB(NOW(), INTERVAL 120 MINUTE)')
  || !preg_match('/thread_id = ([0-9]+)/',$query,$thread)) throw new RuntimeException('Selected query changed.');
 $since=preg_match('/created_on <= FROM_UNIXTIME\(([0-9]+)\)/',$query,$cutoff)?(int)$cutoff[1]:null;
 $input=$GLOBALS['input'];$prefix=substr($prefix,0,-1);
 $rows=array_values(array_filter($input['events'],fn($event)=>$event['board']===$board
  && $event['parent']===(int)$thread[1] && $event['browser']===$browser && str_starts_with($event['ip'],$prefix)
  && $event['created_at']>$input['event_at']-7200 && ($since===null || $event['created_at']<=$since)));
 $GLOBALS['lookups'][]=['board'=>$board,'parent'=>(int)$thread[1],'browser'=>$browser,'prefix'=>$prefix,'since'=>$since];
 return new Cursor($rows);
}
function mysql_num_rows($cursor){return count($cursor->rows);}
set_error_handler(function(){throw new RuntimeException('Unexpected isolated-source warning.');});
$now=1700000000;
$event=['id'=>1,'board'=>'g','parent'=>123,'browser'=>'1deadbeef','ip'=>'192.0.2.99','created_at'=>$now-1];
$base=['request_at'=>$now,'activity_at'=>$now,'event_at'=>$now,'ip'=>'192.0.2.10','parent'=>123,'browser'=>'1deadbeef','actor'=>null,'events'=>[$event]];
$actors=[null];
foreach([0,7199,7200,86400] as $password)foreach([0,899,900,901,1799,1800,7199,7200] as $network)foreach([0,1] as $posts)foreach([0,10] as $score)
 $actors[]=['password_age'=>$password,'network_age'=>$network,'posts'=>$posts,'score'=>$score,'verified'=>0,'reports'=>0,'pending'=>0];
foreach([0,900,7200] as $network)foreach([0,10] as $score)
 $actors[]=['password_age'=>0,'network_age'=>$network,'posts'=>0,'score'=>$score,'verified'=>1,'reports'=>0,'pending'=>0];
foreach([899,900,901] as $network)
 $actors[]=['password_age'=>0,'network_age'=>$network,'posts'=>0,'score'=>0,'verified'=>0,'reports'=>0,'pending'=>1];
foreach([5,6,20] as $reports)foreach([0,1800] as $network)
 $actors[]=['password_age'=>7200,'network_age'=>$network,'posts'=>0,'score'=>0,'verified'=>0,'reports'=>$reports,'pending'=>8];
$cases=[];
function record($input) {
 $GLOBALS['input']=$input;$GLOBALS['hits']=0;$GLOBALS['lookups']=[];$GLOBALS['rangeban_bypass']=false;$GLOBALS['captcha_bypass']=false;
 $_SERVER=['REMOTE_ADDR'=>$input['ip'],'HTTP_X_GEO_ASN'=>'0','REQUEST_TIME'=>$input['request_at']];
 $state=null;
 if($input['actor']!==null) {
  $a=$input['actor'];$state=new UserPwd();$state->now=$input['activity_at'];$state->creation_ts=$state->now-$a['password_age'];$state->mask_ts=$state->now-$a['network_age'];
  $state->post_count=$a['posts'];$state->report_count=$a['reports'];$state->verified_level=$a['verified'];$state->ip_change_score=$a['score'];$state->action_buffer=$a['pending'];
 }
 $blocked=false;
 try {spam_filter_post_ip($state,$input['parent'],false);} catch(OwnedBlocked $e){$blocked=true;}
 if($GLOBALS['hits']!==($blocked?1:0))throw new RuntimeException('Unexpected hit side effect.');
 $input['blocked']=$blocked;$input['lookups']=$GLOBALS['lookups'];$GLOBALS['cases'][]=$input;
}
foreach($actors as $actor)foreach([-1,0,899,900,901,7199,7200,7201] as $age)
 record(array_replace($base,['actor'=>$actor,'events'=>[array_replace($event,['created_at'=>$now-$age])]]));
foreach([null,0,123,124] as $parent)foreach(['0deadbeef','1deadbeef','1badc0ffe'] as $browser)
 record(array_replace($base,['parent'=>$parent,'browser'=>$browser]));
foreach(['g','a'] as $board)foreach(['192.0.9.99','192.1.2.99'] as $ip)foreach([0,123,124] as $parent)
 record(array_replace($base,['events'=>[array_replace($event,['board'=>$board,'ip'=>$ip,'parent'=>$parent])]]));
foreach([-1,0,1] as $activity)foreach([-1,0,1] as $clock)
 record(array_replace($base,['activity_at'=>$now+$activity,'event_at'=>$now+$clock,'actor'=>['password_age'=>0,'network_age'=>900,'posts'=>1,'score'=>0,'verified'=>0,'reports'=>0,'pending'=>0],
  'events'=>[array_replace($event,['created_at'=>$now-900])]]));
record(array_replace($base,['parent'=>0,'events'=>[array_replace($event,['parent'=>0])]]));
$payload=['files'=>$hashes,'methods'=>$methods,'scope'=>'Only automatic stage; configured policy and auth bypass are disabled. SQL callback models the selected candidate query, not a database engine.','cases'=>[]];
$json=json_encode($payload,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR);
$rows=array_map(fn($case)=>'        '.json_encode($case,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),$cases);
$json=str_replace('"cases": []','"cases": ['."\n".implode(",\n",$rows)."\n    ]",$json)."\n";
if(count($cases)<1000 || strlen($json)>4*1024*1024)throw new RuntimeException('Unexpected corpus bounds.');
if(in_array('--check',$argv,true)){if(file_get_contents($argv[2])!==$json)throw new RuntimeException('Automatic fixture differs.');}
else file_put_contents($argv[2],$json);
printf("Automatic network fixture matches %d cases.\n",count($cases));
