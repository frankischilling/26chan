<?php
namespace OwnedRequestFiles;

$fixture = json_decode(\file_get_contents(__DIR__.'/../fixtures/janitor-file-effects-source.json'), true, 8, JSON_THROW_ON_ERROR);
if ($fixture['sources'] !== [
    'admin.php'=>'4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb',
    'reports/ReportQueue.php'=>'e1eaa54bd229f0d718d026cb11a757576f67f02a86e21c4bf5ff051bbfbac0d1',
]) throw new \RuntimeException('File-effect source provenance differs');
foreach ($fixture['php'] as $name=>$body) {
    if (!in_array($name,['capture','amend','blacklist','clear'],true) || strlen($body)>8192) throw new \RuntimeException('Unexpected source branch');
}
define('SALTFILE','/owned/salt');
define('IMG_DIR','/owned/public/');
define('THUMB_DIR','/owned/thumb/');
define('BANIMG_ROOT','/owned/private/');
define('BANTHUMB_DIR','/owned/private-thumb/');
function file_get_contents($path) {
    if (!in_array($path,['/owned/salt','/www/keys/legacy.salt'],true)) throw new \RuntimeException('Unexpected file read');
    return 'synthetic-file-policy-seed';
}
function copy($source,$destination) {
    $GLOBALS['trace']['copies'][]=['source'=>$source,'destination'=>$destination];
    return $GLOBALS['input']['copy_succeeds'];
}
function unlink($path) { $GLOBALS['trace']['unlinks'][]=$path; return true; }
function mysql_result($result,$row,$column) { return $result[0]; }
function mysql_insert_id($connection) { return 101; }
function mysql_global_call($query,...$args) {
    if (str_starts_with($query,'SELECT COUNT(*) FROM reports ')) {
        $GLOBALS['trace']['report_target']=$args;
        return [$GLOBALS['input']['illegal'] ? 1 : 0];
    }
    if (str_starts_with($query,'INSERT INTO blacklist(')) {
        $GLOBALS['trace']['blacklists'][]=['ban'=>str_contains($query,"'1','-1'"),'arguments'=>$args];
        return true;
    }
    throw new \RuntimeException('Unexpected SQL call');
}
function error($message) { throw new \RuntimeException($message); }
set_error_handler(function($level,$message) {
    if (str_contains($message,'Undefined variable $has_approved')) $GLOBALS['trace']['undefined_approved']=true;
    elseif (str_contains($message,'Undefined property: stdClass::$raw_md5')) $GLOBALS['trace']['undefined_digest']=true;
    else throw new \RuntimeException($message);
    return true;
});
function run_case($input,$mutation=false) {
    global $fixture;
    $GLOBALS['input']=$input;
    $GLOBALS['trace']=['copies'=>[],'unlinks'=>[],'blacklists'=>[],'report_target'=>null,'undefined_approved'=>false,'undefined_digest'=>false];
    $board='g';
    $_POST=['no'=>7];
    $_COOKIE=['4chan_auser'=>'Owned moderator'];
    $bres=['save_post'=>$input['capture_policy']];
    $post=['no'=>7,'tim'=>123,'ext'=>$input['file'] ? '.png' : '', 'md5'=>$input['digest_present'] ? str_repeat('a',32) : ''];
    $capture=$fixture['php']['capture'];
    if ($mutation) {
        $capture=str_replace('(mysql_result($illegal, 0, 0) == 0)','true',$capture,$changed);
        if ($changed!==1) throw new \RuntimeException('Illegal-retention control lost its source target');
    }
    eval('namespace OwnedRequestFiles;'.$capture);
    $capture_raw=array_key_exists('raw_md5',$post);
    $spost=$post;
    $post=(object)$post;
    $request=['board'=>'g','janitor'=>'Owned janitor'];
    $template=['save_post'=>$input['review_policy'],'blacklist'=>$input['blacklist'],'publicreason'=>'Owned reason'];
    $req_amend=$input['amended'];
    $warn=$input['warning_action'];
    $gcon=null;
    eval('namespace OwnedRequestFiles;'.$fixture['php']['amend']);
    eval('namespace OwnedRequestFiles;'.$fixture['php']['blacklist']);
    $result=$GLOBALS['trace'];
    return ['capture_raw'=>$capture_raw,'review_raw'=>property_exists($post,'raw_md5'),
        'copy_count'=>count($result['copies']),'unlink_count'=>count($result['unlinks']),
        'blacklist_count'=>count($result['blacklists']),
        'blacklist_ban'=>$result['blacklists'][0]['ban'] ?? null,
        'report_target'=>$result['report_target'],'undefined_approved'=>$result['undefined_approved'],
        'undefined_digest'=>$result['undefined_digest']];
}
$cases=[];
foreach (['none','json_only','everything'] as $capture)
foreach (['none','json_only','everything'] as $review)
foreach ([false,true] as $illegal)
foreach ([false,true] as $file)
foreach ([false,true] as $digest)
foreach ([false,true] as $copied)
foreach ([false,true] as $amended)
foreach ([false,true] as $warn)
foreach (['none','image','rejectimage'] as $blacklist) {
    $input=['capture_policy'=>$capture,'review_policy'=>$review,'illegal'=>$illegal,'file'=>$file,
        'digest_present'=>$digest,'copy_succeeds'=>$copied,'amended'=>$amended,'warning_action'=>$warn,'blacklist'=>$blacklist];
    $cases[]=['input'=>$input,'expected'=>run_case($input)];
}
$control=['capture_policy'=>'everything','review_policy'=>'none','illegal'=>true,'file'=>true,
    'digest_present'=>true,'copy_succeeds'=>true,'amended'=>false,'warning_action'=>false,'blacklist'=>'none'];
if (run_case($control)['copy_count']!==0 || run_case($control,true)['copy_count']!==2) {
    throw new \RuntimeException('Illegal-report exclusion control failed');
}
$clear=[];
foreach ([false,true] as $approved) {
    $GLOBALS['trace']=['unlinks'=>[]];
    $request=['board'=>'g']; $post=(object)['ext'=>'.png']; $hash='synthetic';
    $ban_image_dir='/owned/private/'; $ban_thumb_dir='/owned/thumb/'; $has_approved=$approved;
    eval('namespace OwnedRequestFiles;'.$fixture['php']['clear']);
    $clear[]=['approved'=>$approved,'unlinks'=>$GLOBALS['trace']['unlinks']];
}
$encoded=json_encode(['cases'=>$cases,'clear'=>$clear],JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
$path=__DIR__.'/../crates/domain/tests/fixtures/janitor-file-effects.json';
if (in_array('--write',$argv,true)) file_put_contents($path,$encoded);
elseif (\file_get_contents($path)!==$encoded) throw new \RuntimeException('Janitor file-effect decisions differ');
printf("Janitor file effects match %d cases, approval cleanup and illegal-retention control.\n",count($cases));
