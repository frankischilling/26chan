<?php
// Execute a pinned posting prefix and selected pure UserPwd methods only.
// Database, logging, response and random effects are bounded local recording stubs.
if (PHP_SAPI !== 'cli' || $argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
 throw new RuntimeException('Usage: php -n extract-post-history-reference.php SOURCE OUTPUT [--check]');
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
$start=strpos($source,'      // FLOOD CHECK');
$end=strpos($source,'    /**',$start);
$prefix=substr($source,$start,$end-$start);
$prefixHash='745b9b623820488740874d3fe401d019bb92ea48abb1e1cdbff967e3cec75677';
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
$methods=['pwdLifetime','maskLifetime','verifiedLevel','isUserKnown','isUserKnownOrVerified','postCount','reportCount','ipChangeScore','envChanged','isNew'];
$class='class Actor { public $now,$creation_ts,$mask_ts,$env_ts,$verified_level,$post_count,$report_count,$action_buffer,$ip_change_score;';
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
const S_IPRANGE_BLOCKED='range'; const S_IPRANGE_BLOCKED_TEMP='temporary'; const S_IPRANGE_BLOCKED_L1='level1';
class Stop extends \RuntimeException {}
function spam_filter_is_pwd_blocked($pwd,$type,$hours) {
 if($pwd!=='owned' || $type!=='block_bm_bot' || $hours!==24) {throw new \RuntimeException('Unexpected history lookup.');}
 $GLOBALS['owned_calls'][]='edge_history';return $GLOBALS['owned_edge_history'];
}
function spam_filter_is_likely_automated($cache) {$GLOBALS['owned_calls'][]='automated';return $GLOBALS['owned_automated'];}
function spam_filter_has_country_changed($pwd) {
 if($pwd!=='owned') {throw new \RuntimeException('Unexpected country lookup.');}
 $GLOBALS['owned_calls'][]='country_history';return $GLOBALS['owned_country_history'];
}
function spam_filter_format_http_headers(...$args) {return [];}
function log_spam_filter_trigger($name,...$args) {$GLOBALS['owned_events'][]='log:'.$name;}
function write_to_event_log($name,...$args) {$GLOBALS['owned_events'][]='record:'.$name;}
function error($message) {
 if($message!==S_IPRANGE_BLOCKED.' '.S_IPRANGE_BLOCKED_TEMP.S_IPRANGE_BLOCKED_L1) {throw new \RuntimeException('Unexpected error.');}
 $GLOBALS['owned_rejected']=true;throw new Stop();
}
STUBS);
// getPwd is an isolated fixed identifier; it is never a credential or SQL value.
// The extracted methods above remain unchanged; the subclass supplies only this accessor.
eval('namespace OwnedLate; class HistoryActor extends Actor {public function getPwd(){return "owned";}}');
eval('namespace OwnedLate; function run($captcha_bypass,$userpwd) { $owned_board="a";$resto=42;$host="192.0.2.1";$com="owned";$country="US";$insfile="owned";$ext=".png";$_threat_score=0;$_req_sig="";$memcached=null;if(!$captcha_bypass){'.$body.'}');
$now=10000000;$cases=[];
$actors=[null];
foreach([0,1,86399,86400] as $age) foreach([0,19,20] as $posts) foreach([0,1] as $pending)
 foreach([0,1] as $verified) foreach([false,true] as $changed) {
 $actors[]=['age'=>$age,'posts'=>$posts,'pending'=>$pending,'verified'=>$verified,'changed'=>$changed];
}
foreach($actors as $actor) foreach([null,0,29,99,"","not-numeric","0","29"] as $edge) foreach([false,true] as $edgeHistory)
 foreach([false,true] as $automated) foreach([false,true] as $countryHistory) foreach([false,true] as $bypass) {
 $state=null;
 if($actor!==null) {
  $state=new \OwnedLate\HistoryActor();$state->now=$now;$state->creation_ts=$now-$actor['age'];$state->mask_ts=$state->creation_ts;
  $state->env_ts=$actor['changed']?$now:$now-1;$state->post_count=$actor['posts'];$state->report_count=0;
  $state->action_buffer=$actor['pending'];$state->verified_level=$actor['verified'];$state->ip_change_score=0;
 }
 $_SERVER=[];if($edge!==null){$_SERVER['HTTP_X_BOT_SCORE']=$edge;}
 $GLOBALS['owned_edge_history']=$edgeHistory;$GLOBALS['owned_automated']=$automated;$GLOBALS['owned_country_history']=$countryHistory;
 $GLOBALS['owned_events']=[];$GLOBALS['owned_calls']=[];$GLOBALS['owned_rejected']=false;
 try{\OwnedLate\run($bypass,$state);}catch(\OwnedLate\Stop $e){}
 $cases[]=compact('actor','edge','edgeHistory','automated','countryHistory','bypass')+
  ['rejected'=>$GLOBALS['owned_rejected'],'events'=>$GLOBALS['owned_events'],'calls'=>$GLOBALS['owned_calls']];
}
$result=['source_sha256'=>$sourceHash,'prefix_sha256'=>$prefixHash,'actor_sha256'=>$userHash,'php_version'=>PHP_VERSION,'now'=>$now,'cases'=>$cases];
$json=json_encode($result,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
if(count($cases)!==12416 || strlen($json)>12*1024*1024) {throw new RuntimeException('Bounded corpus exceeded.');}
if($argc===4) {if(file_get_contents($argv[2])!==$json){throw new RuntimeException('Reference differs.');}}
else {file_put_contents($argv[2],$json);}
echo count($cases)." isolated late-post history cases.\n";
