<?php
// Execute isolated decisions with owned scalar inputs. No SQL, keys or sessions.
$fixture=json_decode(file_get_contents(__DIR__.'/../fixtures/pass-policy-source.json'),true,512,JSON_THROW_ON_ERROR);
date_default_timezone_set('UTC');
define('PASS_TIMEOUT',900);
define('S_PASSEXPIRED','expired');define('S_PASSDISABLED','disabled');define('S_PASSINUSE','inuse');
function clear_pass_cookies() {}
function error($text) { throw new RuntimeException($text); }
final class PassPolicyFixture {
 const ERR_IN_USE='inuse:%s',ERR_EXPIRED='disabled:1',ERR_REFUNDED='disabled:2',ERR_DISPUTED='disabled:3',ERR_REVOKED_SPAM='disabled:4',ERR_REVOKED_ILLEGAL='disabled:5';
 private $result='continue';
 private function error($text) { throw new RuntimeException($text); }
 private function clear_cookies() {}
 private function pretty_duration($seconds) { return (string)$seconds; }
 private function convert_new_pass_status($id,$pin) { $this->result='activate'; }
 private function convert_delayed_pass_status($id,$pin) { $this->result='activate_year'; }
 function login($pass,$time_now,$host,$fragment) {
  $this->result='continue';$long_ip=ip2long($host);$pin='owned';
  try { eval($fragment); return $this->result; } catch(RuntimeException $e) { return $e->getMessage(); }
 }
}
$runner=new PassPolicyFixture();$cases=[];$now=2000000000;
$peers=[['192.0.2.1','192.0.2.1'],['192.0.2.1','192.0.2.2'],['192.0.2.1','192.0.3.1'],['192.0.2.1','198.51.100.1'],['0.0.0.0','192.0.2.1']];
foreach(range(0,7) as $status) foreach([-1,0,899,900,1799,1800,1801] as $age) foreach($peers as [$last,$host]) {
 $pass=['status'=>$status,'last_used'=>gmdate('Y-m-d H:i:s',$now-$age),'last_ip'=>$last,'pending_id'=>'owned','user_hash'=>'OWNEDTOKEN'];
 $cases[]=['route'=>'login','status'=>$status,'now'=>$now,'last_used'=>$now-$age,'last_ip'=>$last,'peer'=>$host,'expected'=>$runner->login($pass,$now,$host,$fixture['login_php'])];
 foreach([-1,0,1] as $expiry) {
  $res=$pass+['expiration_date'=>$now+$expiry];$time=$now;
  // The posting function has a separate source constant with the same name.
  try { eval(str_replace('PASS_TIMEOUT',(string)$fixture['posting_timeout'],$fixture['posting_php']));$expected='continue'; }
  catch(RuntimeException $e) { $expected=$e->getMessage(); }
  $cases[]=['route'=>'posting','status'=>$status,'now'=>$now,'last_used'=>$now-$age,'last_ip'=>$last,'peer'=>$host,'expires'=>$now+$expiry,'expected'=>$expected];
 }
}
$path=__DIR__.'/../fixtures/pass-policy-cases.json';$encoded=json_encode($cases,JSON_THROW_ON_ERROR)."\n";
if(in_array('--write',$argv,true))file_put_contents($path,$encoded);
else if(file_get_contents($path)!==$encoded)throw new RuntimeException('Pass policy source differs');
echo count($cases)." isolated Pass policy cases passed.\n";
