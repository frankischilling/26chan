<?php
// Only the authenticated-staff early-return path is executed. No Pass data,
// deployment configuration, original application or private list is loaded.
if ($argc < 3) { fwrite(STDERR,"Usage: php extract-staff-network-bypass-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$hashes=[
 'lib/auth.php'=>'98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019',
 'lib/postfilter.php'=>'d0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207',
];
if (($argv[3]??'')!=='--case') {
 $cases=[];
 foreach(['janitor','mod','manager','admin'] as $role) foreach([0,1] as $captcha) foreach([0,1] as $local) {
  $process=proc_open([PHP_BINARY,'-n',__FILE__,$argv[1],$argv[2],'--case',$role,(string)$captcha,(string)$local],
   [1=>['pipe','w'],2=>['pipe','w']],$pipes);
  if(!is_resource($process)) throw new RuntimeException('Cannot start isolated source case.');
  $stdout=stream_get_contents($pipes[1]);$stderr=stream_get_contents($pipes[2]);
  fclose($pipes[1]);fclose($pipes[2]);
  if(proc_close($process)!==0 || $stderr!=='') throw new RuntimeException('Selected source path failed: '.$stderr);
  $cases[]=json_decode($stdout,true,512,JSON_THROW_ON_ERROR);
 }
 $json=json_encode(['source_sha256'=>$hashes,'scope'=>'Authenticated staff early return only; local authority is synthetic and is not inferred from a network address. Pass validation is not executed.','cases'=>$cases],JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
 if(in_array('--check',$argv,true)) { if(file_get_contents($argv[2])!==$json) throw new RuntimeException('Staff bypass fixture differs.'); }
 else file_put_contents($argv[2],$json);
 echo "Staff network bypass fixture matches 16 source cases.\n"; exit;
}
if($argc!==7 || !in_array($argv[4],['janitor','mod','manager','admin'],true)
 || !in_array($argv[5],['0','1'],true) || !in_array($argv[6],['0','1'],true)) throw new RuntimeException('Invalid isolated staff case.');
foreach($hashes as $file=>$hash) {
 $source=file_get_contents($argv[1].'/'.$file);
 if(hash('sha256',$source)!==$hash) throw new RuntimeException('Audited source differs.');
 if($file==='lib/auth.php') {
  if(preg_match('/\$levelorderf = array\(\s*\x27janitor\x27 => 1,\s*\x27mod\x27\s*=> 10,\s*\x27manager\x27 => 20,\s*\x27admin\x27\s*=> 50\s*\);/',$source,$rank)!==1)
   throw new RuntimeException('Source rank map differs.');
  eval($rank[0]);
 }
 foreach($file==='lib/auth.php'?['has_level','valid_captcha_bypass']:['spam_filter_post_ip'] as $name) {
  if(preg_match_all('/^function '.preg_quote($name,'/').'\(.*?^}/ms',$source,$matches)!==1 || strlen($matches[0][0])>16384)
   throw new RuntimeException('Selected source function differs.');
  eval($matches[0][0]);
 }
}
unset($source,$matches,$rank);
define('YES',true);define('CAPTCHA',(int)$argv[5]);
$auth=['level'=>$argv[4]];$local=$argv[6]==='1';$_COOKIE=[];
function is_local_auth() { return $GLOBALS['local']; }
function mysql_global_call(...$args) { throw new RuntimeException('Bypassed path reached policy SQL.'); }
class GeoIP2 { static function get_asn($ip) { throw new RuntimeException('Bypassed path reached ASN lookup.'); } }
set_error_handler(function(){throw new RuntimeException('Unexpected selected-source warning.');});
$valid=valid_captcha_bypass();
$blocked=spam_filter_post_ip(null,0,true);
echo json_encode(['role'=>$argv[4],'captcha'=>(bool)CAPTCHA,'local_auth'=>$local,'valid'=>$valid,
 'captcha_bypass'=>$captcha_bypass,'range_bypass'=>$rangeban_bypass,'network_blocked'=>$blocked],JSON_THROW_ON_ERROR);
