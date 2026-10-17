<?php
// Execute a pinned posting prefix and selected pure UserPwd methods only.
// Database, logging, response and random effects are bounded local recording stubs.
if (PHP_SAPI !== 'cli' || $argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
 throw new RuntimeException('Usage: php -n extract-late-post-reference.php SOURCE OUTPUT [--check]');
}
if (!extension_loaded('tokenizer')) { throw new RuntimeException('Enable the tokenizer extension with -d extension=tokenizer.'); }
if (PHP_VERSION !== '8.4.24' || PHP_INT_SIZE !== 8) { throw new RuntimeException('Use qualified 64-bit PHP 8.4.24.'); }
$root=realpath($argv[1]);
if (!$root || !realpath(dirname($argv[2]))) { throw new RuntimeException('Missing source/output directory.'); }
$outputParent=realpath(dirname($argv[2]));$existing=realpath($argv[2]);
if ($outputParent===$root || str_starts_with($outputParent,$root.DIRECTORY_SEPARATOR) || ($existing && str_starts_with($existing,$root.DIRECTORY_SEPARATOR))) { throw new RuntimeException('Write fixtures outside the reference tree.'); }
$source=file_get_contents($root.'/imgboard.php');
$sourceHash='caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445';
if (hash('sha256',$source)!==$sourceHash) { throw new RuntimeException('Posting source differs.'); }
$start=strpos($source,'    $_threat_score = 0;');
$end=strpos($source,'      // FLOOD CHECK',$start);
$prefix=substr($source,$start,$end-$start);
$prefixHash='7259bc1e5c1688a0da6c651318be19b562ad97ee6b9ad64547935ddf9e6af8f5';
if (hash('sha256',$prefix)!==$prefixHash) { throw new RuntimeException('Audited prefix boundary differs.'); }
// BOARD_DIR is constant within each invocation. Substitute only its identifier,
// allowing synthetic boards in one isolated process; strings/comments stay intact.
$tokens=token_get_all('<?php '.$prefix); $body='';
foreach ($tokens as $token) {
 if (is_array($token) && $token[0]===T_OPEN_TAG) { continue; }
 $body.=is_array($token) ? (($token[0]===T_STRING && $token[1]==='BOARD_DIR')?'$owned_board':$token[1]) : $token;
}
$user=file_get_contents($root.'/lib/userpwd.php');
$userHash='0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26';
if(hash('sha256',$user)!==$userHash) { throw new RuntimeException('Activity source differs.'); }
$methods=['pwdLifetime','maskLifetime','verifiedLevel','isUserKnown','isUserKnownOrVerified','postCount','reportCount','ipChangeScore','maskChanged','isNew'];
$class='class Actor { public $now,$creation_ts,$mask_ts,$verified_level,$post_count,$report_count,$action_buffer,$ip_change_score;';
foreach(['IP_CHANGE_DELAY','IP_CHANGE_MASK_VAL','A_POST','A_REPORT'] as $name) {
 if(!preg_match('/const\s+'.$name.'\s*=\s*([0-9]+)\s*;/',$user,$m)) { throw new RuntimeException('Missing audited constant.'); }
 $class.='const '.$name.'='.$m[1].';';
}
$tokens=token_get_all($user); $found=[];
for($i=0;$i<count($tokens);++$i) {
 if(!is_array($tokens[$i]) || $tokens[$i][0]!==T_FUNCTION) { continue; }
 $begin=$i; do {++$i;} while(is_array($tokens[$i]) && $tokens[$i][0]===T_WHITESPACE);
 if(!is_array($tokens[$i]) || !in_array($tokens[$i][1],$methods,true)) {continue;}
 $name=$tokens[$i][1]; if(isset($found[$name])) {throw new RuntimeException('Repeated method.');}
 $part='';$depth=0;$opened=false;
 for($j=$begin;$j<count($tokens);++$j) {
  $token=$tokens[$j];$part.=is_array($token)?$token[1]:$token;
  if($token==='{') {++$depth;$opened=true;} elseif($token==='}' && --$depth===0 && $opened) {break;}
 }
 $class.='public '.$part; $found[$name]=true;
}
if(count($found)!==count($methods)) {throw new RuntimeException('Incomplete audited methods.');}
$class.='}';
eval('namespace OwnedLate; '.$class);
eval(<<<'STUBS'
namespace OwnedLate;
const S_IPRANGE_BLOCKED_IMG='image'; const S_IPRANGE_BLOCKED_TEMP='temporary'; const S_IPRANGE_BLOCKED_L1='level1'; const S_IPRANGE_BLOCKED='plain';
class Stop extends \RuntimeException {}
function mt_rand($min,$max) {
 if($min!==0 || !in_array($max,[1,9],true)) {throw new \RuntimeException('Unexpected random range.');}
 $GLOBALS['owned_ranges'][]=$max+1;
 $list=$max===9?'owned_digits':'owned_coins';
 if(!$GLOBALS[$list]) {throw new \RuntimeException('Random tape exhausted.');}
 $value=array_shift($GLOBALS[$list]);
 if($value<0 || $value>$max) {throw new \RuntimeException('Invalid bounded draw.');}
 return $value;
}
function spam_filter_get_threat_score($country,$op,$multipart) {$GLOBALS['owned_score_calls']++;return $GLOBALS['owned_score'];}
function spam_filter_format_http_headers(...$args) {return [];}
function log_spam_filter_trigger($name,...$args) {$GLOBALS['owned_events'][]=$name;}
function show_post_successful_fake($parent) {$GLOBALS['owned_outcome']='quiet';}
function error($message) {
 if($message!==S_IPRANGE_BLOCKED_IMG.' '.S_IPRANGE_BLOCKED_TEMP.S_IPRANGE_BLOCKED_L1) {throw new \RuntimeException('Unexpected error branch.');}
 $GLOBALS['owned_outcome']='image_range_rejected';throw new Stop();
}
STUBS);
eval('namespace OwnedLate; function run($owned_board,$resto,$has_image,$captcha_bypass,$userpwd,$country,$_thread_sub,$_thread_com,$browser_id) { $com="Owned body";$insfile="owned";$ext=".png";$host="192.0.2.1";$_req_sig="";'.$body.'} }');
$now=10000000;
$actors=[null,
 ['age'=>60,'network'=>60,'posts'=>0,'pending'=>0,'verified'=>0,'churn'=>0],
 ['age'=>21600,'network'=>21600,'posts'=>0,'pending'=>0,'verified'=>0,'churn'=>0],
 ['age'=>60,'network'=>60,'posts'=>11,'pending'=>0,'verified'=>0,'churn'=>0],
 ['age'=>60,'network'=>60,'posts'=>0,'pending'=>0,'verified'=>1,'churn'=>0],
 ['age'=>60,'network'=>60,'posts'=>10,'pending'=>1,'verified'=>0,'churn'=>0],
 ['age'=>3600,'network'=>0,'posts'=>1,'pending'=>0,'verified'=>0,'churn'=>0],
 ['age'=>259200,'network'=>0,'posts'=>11,'pending'=>0,'verified'=>0,'churn'=>10],
 ['age'=>60,'network'=>0,'posts'=>0,'pending'=>1,'verified'=>0,'churn'=>0]];
$scenarios=[
 ['g','/aicg/',''], ['vg','','/lolg/'], ['vg','','/overwatch'], ['vg','/bag/',''],
 ['vg','granblue',''],['v','gamesdonequick',''],['vg','/zzz/',''],['vg','/funkg/',''],
 ['a','ordinary',''],['f','ordinary',''],['fa','Workwear General',''],
 ['vg','/bag/ granblue /zzz/ /funkg/','/lolg/ /overwatch'],
 ['vg','granblue /zzz/ /funkg/',''],['vg','/zzz/ /funkg/','']
];
$cases=[];
function record($board,$subject,$comment,$actor,$reply,$image,$score,$digit,$coin,$cookie=null,$country='US',$browser='04d2237a2',$bypass=false) {
 global $cases,$now;
 $state=null;
 if($actor!==null) {
  $state=new \OwnedLate\Actor();$state->now=$now;$state->creation_ts=$now-$actor['age'];$state->mask_ts=$now-$actor['network'];
  $state->post_count=$actor['posts'];$state->report_count=0;$state->action_buffer=$actor['pending'];$state->verified_level=$actor['verified'];$state->ip_change_score=$actor['churn'];
 }
 $_COOKIE=[];if($cookie!==null){$_COOKIE['_tcs']=$cookie;}
 $GLOBALS['owned_score']=$score;$GLOBALS['owned_score_calls']=0;$GLOBALS['owned_ranges']=[];
 $GLOBALS['owned_digits']=[$digit,9];$GLOBALS['owned_coins']=[$coin];$GLOBALS['owned_outcome']='continue';$GLOBALS['owned_events']=[];
 try{\OwnedLate\run($board,$reply?42:0,$image,$bypass,$state,$country,$subject,$comment,$browser);}catch(\OwnedLate\Stop $e){}
 $cases[]=compact('board','subject','comment','actor','reply','image','score','digit','coin','cookie','country','browser','bypass')+
  ['outcome'=>$GLOBALS['owned_outcome'],'events'=>$GLOBALS['owned_events'],'draws'=>$GLOBALS['owned_ranges'],'score_calls'=>$GLOBALS['owned_score_calls']];
}
foreach($scenarios as [$board,$subject,$comment]) foreach($actors as $actor)
 foreach([0.0,0.08,0.09,0.15,0.31,0.32] as $score) foreach([0,1,3,4,5,9] as $digit) foreach([0,1] as $coin) {
 record($board,$subject,$comment,$actor,true,true,$score,$digit,$coin);
 }
foreach(['g','vg','v','f','a'] as $board) foreach([false,true] as $reply) foreach([false,true] as $image)
 foreach($actors as $actor) foreach([0.08,0.09,0.31,0.32] as $score) foreach([0,3,4,5,9] as $digit) {
 record($board,'/aicg/ /bag/ granblue /zzz/ /funkg/ gamesdonequick','/lolg/ /overwatch',$actor,$reply,$image,$score,$digit,1);
 }
foreach([null,'','x.America/Bogota.100.33','x.america/Bogota.100.33'] as $cookie)
 foreach(['US','JP'] as $country) foreach([null,'','other','04d2237a2'] as $browser)
 foreach([false,true] as $bypass) foreach($actors as $actor) {
 record('vg','/bag/','',$actor,true,true,0.0,0,0,$cookie,$country,$browser,$bypass);
 }
foreach(['a','f'] as $board) foreach([0.09,0.31] as $score) foreach([2,3] as $digit) foreach([0,1] as $coin) {
 record($board,'ordinary','',$actors[1],false,true,$score,$digit,$coin);
}
// Equal creation and activity timestamps suppress the mask-change event.
foreach([0,1] as $age) foreach([0,1] as $posts) foreach([0,1] as $pending) foreach([0.0,0.32] as $score) {
 record('a','ordinary','',['age'=>$age,'network'=>0,'posts'=>$posts,'pending'=>$pending,'verified'=>0,'churn'=>0],true,false,$score,0,0);
}
$result=['source_sha256'=>$sourceHash,'prefix_sha256'=>$prefixHash,'actor_sha256'=>$userHash,'php_version'=>PHP_VERSION,'now'=>$now,'cases'=>$cases];
$json=json_encode($result,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
if(count($cases)!==13280 || strlen($json)>12*1024*1024) {throw new RuntimeException('Bounded corpus exceeded.');}
if($argc===4) {if(file_get_contents($argv[2])!==$json){throw new RuntimeException('Reference differs.');}}
else {file_put_contents($argv[2],$json);}
echo count($cases)." isolated late-post prefix cases.\n";
