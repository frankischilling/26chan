<?php
// Execute only the source timing fragment with synthetic database timestamps.
$fixture=json_decode(file_get_contents(__DIR__.'/../fixtures/duplicate-image-cooldown-source.json'),true,512,JSON_THROW_ON_ERROR);
define('RENZOKU_DUPE',$fixture['seconds']);
define('S_RENZOKU2_DUP','%s');define('S_RENZOKU_PASS',':ordinary');
function sec2hms($seconds,$unused,$unused2) { return (string)$seconds; }
function error($message,$unused) { throw new RuntimeException($message); }
$last_time=2000000000;$dest=null;$cases=[];
foreach([false,true] as $captcha_bypass) foreach([-301,-1,0,1,149,150,151,299,300,301,1000] as $age) {
 $time=$last_time+$age;$remaining=null;
 try { eval($fixture['php']); }
 catch(RuntimeException $e) {$remaining=(int)explode(':',$e->getMessage())[0];}
 $cases[]=['bypass'=>$captcha_bypass,'age'=>$age,'remaining'=>$remaining];
}
$encoded=json_encode($cases,JSON_THROW_ON_ERROR)."\n";
$path=__DIR__.'/../fixtures/duplicate-image-cooldown-cases.json';
if(in_array('--write',$argv,true)) file_put_contents($path,$encoded);
else if(file_get_contents($path)!==$encoded) throw new RuntimeException('Duplicate-image cooldown source differs');
echo count($cases)." source duplicate-image cooldown cases passed.\n";
