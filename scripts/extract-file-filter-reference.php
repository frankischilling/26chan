<?php
// Selected, hash-pinned functions only. No application, operator list or key is run.
if ($argc < 3) { fwrite(STDERR, "Usage: php extract-file-filter-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$hashes = [
    'imgboard.php' => 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'lib/postfilter.php' => 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207',
];
if (($argv[3] ?? '') !== '--group') {
    $exact = []; $prior = [];
    foreach (['g','b','test'] as $board) foreach ([0,1] as $worksafe) {
        $process = proc_open([PHP_BINARY, '-n', __FILE__, $argv[1], $argv[2], '--group', $board, (string)$worksafe],
            [1=>['pipe','w'], 2=>['pipe','w']], $pipes);
        if (!is_resource($process)) throw new RuntimeException('Cannot start isolated fixture group.');
        $stdout = stream_get_contents($pipes[1]); $stderr = stream_get_contents($pipes[2]);
        fclose($pipes[1]); fclose($pipes[2]);
        if (proc_close($process)!==0 || $stderr!=='') throw new RuntimeException('Selected source fixture failed: '.$stderr);
        $group = json_decode($stdout,true,512,JSON_THROW_ON_ERROR);
        array_push($exact,...$group['exact_cases']); array_push($prior,...$group['prior_cases']);
    }
    $payload=['source_sha256'=>$hashes,'scope'=>'Selected calls use synthetic SQL and effect collectors. No overlapping same-digest rules or operator data are imported.',
        'exact_cases'=>[],'prior_cases'=>[]];
    $json=json_encode($payload,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
    foreach (['exact_cases'=>$exact,'prior_cases'=>$prior] as $key=>$cases) {
        $rows=array_map(fn($case)=>'        '.json_encode($case,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),$cases);
        $json=str_replace('"'.$key.'": []','"'.$key.'": ['."\n".implode(",\n",$rows)."\n    ]",$json);
    }
    if(count($exact)!==2358 || count($prior)!==600 || strlen($json)>4*1024*1024) throw new RuntimeException('Unexpected fixture bounds.');
    if(in_array('--check',$argv,true)) {
        if(file_get_contents($argv[2])!==$json) throw new RuntimeException('File-filter fixture differs.');
    } else file_put_contents($argv[2],$json);
    printf("File-filter fixture matches %d exact and %d prior-ban cases.\n",count($exact),count($prior));
    exit;
}
if ($argc!==6 || !in_array($argv[4],['g','b','test'],true) || !in_array($argv[5],['0','1'],true)) throw new RuntimeException('Invalid fixture group.');
define('BOARD_DIR',$argv[4]); define('DEFAULT_BURICHAN',$argv[5]==='1'); define('TEST_BOARD',BOARD_DIR==='test');
define('S_ANONAME','Anonymous'); define('S_FAILEDUPLOAD','failed-upload'); define('S_DMCAFAIL','dmca-warning');
define('S_BANNED','banned'); define('S_DMCABANREASON','dmca-repeat');
$_SERVER['REMOTE_ADDR']='192.0.2.1';
foreach (['imgboard.php'=>'check_md5_blacklist','lib/postfilter.php'=>'check_for_banned_upload'] as $file=>$name) {
    $source=file_get_contents($argv[1].'/'.$file);
    if(hash('sha256',$source)!==$hashes[$file]) throw new RuntimeException('Audited source differs.');
    if(preg_match_all('/^function '.preg_quote($name,'/').'\(.*?^}/ms',$source,$matches)!==1 || strlen($matches[0][0])>16384)
        throw new RuntimeException('Selected body differs.');
    eval($matches[0][0]);
}
unset($source,$matches);
class Cursor { public function __construct(public array $rows) {} }
class Stop extends RuntimeException { public function __construct(public array $result) { parent::__construct('Fixture exit'); } }
class L { public static function d($board) { return 'example.invalid'; } }
function mysql_num_rows($cursor) { return count($cursor->rows); }
function mysql_fetch_assoc($cursor) { return $cursor->rows[0] ?? false; }
function mysql_fetch_row($cursor) { return isset($cursor->rows[0]) ? array_values($cursor->rows[0]) : false; }
function mysql_global_call($sql,...$parameters) {
    $input=$GLOBALS['input'];
    if(str_contains($sql,'FROM blacklist')) {
        $rows=array_values(array_filter($input['rules'],fn($row)=>$row['active'] && $row['field']==='md5'
            && $row['contents']===$parameters[0] && ($row['boardrestrict']==='' || $row['boardrestrict']===BOARD_DIR
                || (DEFAULT_BURICHAN && $row['boardrestrict']==='_ws_'))));
        if($rows) $GLOBALS['matched']=['rule'=>$rows[0]['id'],'digest'=>$parameters[0]];
        return new Cursor(array_slice($rows,0,1));
    }
    if(str_contains($sql,'FROM banned_users')) {
        if($input['database_failure']) return false;
        if(!str_contains($sql,DEFAULT_BURICHAN?'IN(226,6)':'IN(226)')) throw new RuntimeException('Prior-ban template query differs.');
        return new Cursor([['cnt'=>$input['count_226']+(DEFAULT_BURICHAN?$input['count_6']:0)]]);
    }
    if(str_contains($sql,"SELECT ip FROM user_actions WHERE action = 'fail_dmca'")) {
        if($input['dmca_database_failure']) return false;
        return new Cursor($input['prior_dmca'] ? [['ip'=>ip2long($_SERVER['REMOTE_ADDR'])]] : []);
    }
    if(str_starts_with($sql,'INSERT INTO user_actions')) {
        $GLOBALS['effects'][]=['kind'=>'dmca-warning']; return true;
    }
    throw new RuntimeException('Unexpected selected query.');
}
function error($message,$dest) { throw new Stop(['outcome'=>str_starts_with($message,'Blacklisted md5 - ')?'private-reject':'reject','message'=>$message]); }
function error_redirect($message,$url) { throw new Stop(['outcome'=>'redirect','message'=>$message,'url'=>$url]); }
function show_post_successful_fake($parent) { throw new Stop(['outcome'=>'quiet','parent'=>$parent]); }
function auto_ban_poster($name,$days,$global,$private,$public='',$filter=false,$password=null,$pass=null) {
    if($password!==null || $pass!==null || $global!==1) throw new RuntimeException('Unexpected synthetic authority.');
    $GLOBALS['effects'][]=['kind'=>'ban','days'=>$days,'filter'=>$filter,'public_reason'=>$public];
}
set_error_handler(function(){ throw new RuntimeException('Unexpected selected-source warning.'); });
$exact=[]; $prior=[]; $normal=str_repeat('11',16); $original=str_repeat('22',16);
$rule=['id'=>1,'active'=>true,'field'=>'md5','contents'=>$normal,'boardrestrict'=>'','ban'=>0,'quiet'=>false,
    'banlength'=>3,'banreason'=>'synthetic reason','description'=>'synthetic owner'];
function record_exact($input) {
    $GLOBALS['input']=$input; $GLOBALS['effects']=[]; $GLOBALS['matched']=null;
    try { check_md5_blacklist($input['normalized'],$input['original'],['resto'=>7,'filename'=>'fixture.png','name'=>'Synthetic'],null); $result=['outcome'=>'allow']; }
    catch(Stop $stop) { $result=$stop->result; }
    $input['matched']=$GLOBALS['matched']; $input['result']=$result; $input['effects']=$GLOBALS['effects']; $GLOBALS['exact'][]=$input;
}
$base=['board'=>BOARD_DIR,'worksafe'=>DEFAULT_BURICHAN,'test_board'=>TEST_BOARD,'normalized'=>$normal,'original'=>$original,'prior_dmca'=>false,'dmca_database_failure'=>false,'rules'=>[]];
foreach([false,true] as $active) foreach(['',BOARD_DIR,'_ws_','other'] as $scope)
foreach([0,1,2] as $mode) foreach([false,true] as $quiet) foreach([false,true] as $repeat)
foreach(['none','normal','original','same-original'] as $match) {
    $hash=match($match){'normal'=>$normal,'original','same-original'=>$original,default=>str_repeat('33',16)};
    record_exact(array_replace($base,['original'=>$match==='same-original'?$normal:$original,'prior_dmca'=>$repeat,
        'rules'=>[array_replace($rule,['active'=>$active,'boardrestrict'=>$scope,'ban'=>$mode,'quiet'=>$quiet,'contents'=>$hash])]]));
}
record_exact(array_replace($base,['normalized'=>null,'rules'=>[array_replace($rule,['contents'=>$original])]]));
record_exact(array_replace($base,['original'=>null,'rules'=>[array_replace($rule,['contents'=>$original])]]));
foreach([-1,0,7] as $days) record_exact(array_replace($base,['rules'=>[
    array_replace($rule,['id'=>2,'contents'=>$original,'ban'=>2]),
    array_replace($rule,['ban'=>1,'banlength'=>$days]),
]]));
foreach([false,true] as $quiet) foreach([$normal,$original] as $hash)
    record_exact(array_replace($base,['prior_dmca'=>true,'dmca_database_failure'=>true,
        'rules'=>[array_replace($rule,['contents'=>$hash,'ban'=>2,'quiet'=>$quiet])]]));
foreach([null,$normal] as $digest) foreach([false,true] as $failure) foreach(range(0,4) as $count226) foreach(range(0,4) as $count6) {
    $input=['board'=>BOARD_DIR,'worksafe'=>DEFAULT_BURICHAN,'normalized'=>$digest,'database_failure'=>$failure,'count_226'=>$count226,'count_6'=>$count6];
    $GLOBALS['input']=$input; $input['blocked']=check_for_banned_upload($digest); $prior[]=$input;
}
echo json_encode(['exact_cases'=>$exact,'prior_cases'=>$prior],JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR);
