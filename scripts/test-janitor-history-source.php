<?php
// Synthetic timestamps only; no original application, SQL adapter or credentials.
$fixture=json_decode(file_get_contents(__DIR__.'/../fixtures/janitor-history-source.json'),true,512,JSON_THROW_ON_ERROR);
$now=2000000000;
$_SERVER['REQUEST_TIME']=$now;
$run=static function($bans) use ($fixture) { eval($fixture['php']); return $ret; };
$cases=[];
foreach ([0,3599,3600,86400,86401,31536000,31536001] as $age) {
 foreach ([null,0,10,11,86400,604800] as $length) {
  foreach ([null,1,90000] as $unban) {
   $created=$now-$age;
   $rows=[['created_on'=>$created,'expires_on'=>$length===null?null:$created+$length,
    'unbanned_on'=>$unban===null?null:$created+$unban,'template_id'=>42]];
   $cases[]=['now'=>$now,'rows'=>$rows,'expected'=>$run($rows)];
  }
 }
}
$rows=[['created_on'=>$now-3600,'expires_on'=>$now-3600,'unbanned_on'=>null,'template_id'=>null],
 ['created_on'=>$now-86400,'expires_on'=>$now+86400,'unbanned_on'=>null,'template_id'=>7],
 ['created_on'=>$now-31536001,'expires_on'=>$now+86400,'unbanned_on'=>null,'template_id'=>8],
 ['created_on'=>$now-50,'expires_on'=>null,'unbanned_on'=>null,'template_id'=>9]];
$cases[]=['now'=>$now,'rows'=>$rows,'expected'=>$run($rows)];
$cases[]=['now'=>$now,'rows'=>[],'expected'=>$run([])];
$path=__DIR__.'/../fixtures/janitor-history-cases.json';
if(in_array('--write',$argv,true)) file_put_contents($path,json_encode($cases,JSON_THROW_ON_ERROR)."\n");
else if(trim(file_get_contents($path))!==json_encode($cases,JSON_THROW_ON_ERROR)) throw new RuntimeException('History source cases differ');
echo count($cases)." source history cases passed.\n";
