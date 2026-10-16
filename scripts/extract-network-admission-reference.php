<?php
// Execute only selected hash-pinned policy/activity bodies with synthetic rows.
// Never load application configuration, cookies, databases or request secrets.
if ($argc < 3) { fwrite(STDERR, "Usage: php extract-network-admission-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Missing reference directory.'); }
$hashes = ['lib/postfilter.php' => 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207',
    'lib/userpwd.php' => '0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26'];
$sources = [];
foreach ($hashes as $file => $hash) {
    $sources[$file] = file_get_contents($root . '/' . $file);
    if (hash('sha256', $sources[$file]) !== $hash) { throw new RuntimeException('Audited source differs.'); }
}
function selected_bodies($source, $names) {
    // Only hash-pinned bodies are accepted. Their closing braces use the
    // declaration indentation; nested braces have strictly deeper indentation.
    $result = '';
    foreach ($names as $name) {
        $pattern = '/^([ \t]*)(?:public[ \t]+)?(function[ \t]+' . preg_quote($name, '/') . '\b.*?^\1})[ \t]*\r?$/ms';
        if (preg_match_all($pattern, $source, $matches) !== 1) {
            throw new RuntimeException('Missing or repeated selected body.');
        }
        $body = $matches[2][0];
        if (strlen($body) > 8192) { throw new RuntimeException('Selected body exceeds bounds.'); }
        $result .= $body . "\n";
    }
    if (strlen($result) > 32768) { throw new RuntimeException('Selected bodies exceed bounds.'); }
    return $result;
}
$methods = ['pwdLifetime', 'maskLifetime', 'verifiedLevel', 'isUserKnown', 'postCount', 'reportCount', 'ipChangeScore'];
$class = 'class UserPwd { public $now=0,$creation_ts=0,$mask_ts=0,$verified_level=0,$post_count=0,$report_count=0,$action_buffer=0,$ip_change_score=0;';
foreach (['IP_CHANGE_DELAY', 'IP_CHANGE_MASK_VAL', 'A_POST', 'A_REPORT'] as $constant) {
    if (!preg_match('/const\s+' . $constant . '\s*=\s*([0-9]+)\s*;/', $sources['lib/userpwd.php'], $match)) { throw new RuntimeException('Missing constant.'); }
    $class .= 'const ' . $constant . '=' . $match[1] . ';';
}
eval($class . selected_bodies($sources['lib/userpwd.php'], $methods) . '}');
eval(selected_bodies($sources['lib/postfilter.php'], ['isIPRangeBanned']));
class OwnedCursor { public $offset=0; public function __construct(public array $rows) {} }
function mysql_global_call($query) {
    $case = $GLOBALS['owned_case']; $now = $case['now'];
    // This stub models only the selected query's candidate set, not a SQL engine.
    if (!str_contains($query, 'ORDER BY lenient ASC') || !str_contains($query, 'active = 1')) { throw new RuntimeException('Candidate query changed.'); }
    $rows = array_values(array_filter($case['rules'], fn($r) => $r['active'] && (!$r['expires_on'] || $r['expires_on'] > $now)
        && (($case['address'] >= $r['range_start'] && $case['address'] <= $r['range_end']) || ($case['asn'] > 0 && $r['asn'] === $case['asn']))));
    usort($rows, fn($a,$b) => $a['lenient'] <=> $b['lenient']);
    return new OwnedCursor($rows);
}
function mysql_fetch_assoc($cursor) { return $cursor->rows[$cursor->offset++] ?? false; }
$now = 1700000000; $ip = ip2long('192.0.2.10');
$baseRule = ['id'=>1,'range_start'=>$ip,'range_end'=>$ip,'asn'=>0,'active'=>1,'expires_on'=>0,'created_on'=>$now-1800,'updated_on'=>0,
    'boards'=>'','ops_only'=>0,'img_only'=>0,'report_only'=>0,'lenient'=>0,'ua_ids'=>''];
$base = ['now'=>$now,'address'=>$ip,'asn'=>0,'board'=>'g','is_sfw'=>false,'is_op'=>false,'has_img'=>false,'is_report'=>false,
    'browser_id'=>null,'req_sig'=>null,'actor'=>null,'rules'=>[$baseRule]];
$actors = [null];
foreach ([[0,0,0,0,0], [86400,86400,0,0,0], [86400,900,3,0,0], [86400,901,3,0,0],
    [86400,0,14,0,0], [86400,0,15,0,0], [0,0,0,1,0], [86400,0,15,0,10], [500,400,1,0,0], [86400,901,2,0,0], [86400,0,15,0,9]] as [$password,$network,$posts,$verified,$score]) {
    $actors[]=['password_age'=>$password,'network_age'=>$network,'posts'=>$posts,'verified'=>$verified,'score'=>$score,'pending'=>0,'reports'=>0];
}
$cases=[];
function record_case($input) {
    global $cases;
    $GLOBALS['owned_case']=$input; $_SERVER['REQUEST_TIME']=$input['now'];
    $options = array_intersect_key($input,array_flip(['board','is_sfw','is_op','has_img','is_report','browser_id','req_sig']));
    if ($input['actor'] !== null) {
        $a=$input['actor']; $state=new UserPwd(); $state->now=$input['now'];
        $state->creation_ts=$state->now-$a['password_age']; $state->mask_ts=$state->now-$a['network_age'];
        $state->post_count=$a['posts']; $state->verified_level=$a['verified']; $state->ip_change_score=$a['score'];
        $state->action_buffer=$a['pending']; $state->report_count=$a['reports']; $options['userpwd']=$state;
    }
    $result=isIPRangeBanned($input['address'],$input['asn'],$options);
    $input['expected_id']=$result ? $result['id'] : null; $cases[]=$input;
}
foreach (range(0,3) as $lenient) foreach ([0,1] as $op) foreach ([0,1] as $image) foreach ($actors as $actor) {
    foreach ([[false,false],[true,false],[false,true],[true,true]] as [$requestOp,$requestImage]) {
        $rule=array_replace($baseRule,['lenient'=>$lenient,'ops_only'=>$op,'img_only'=>$image]);
        record_case(array_replace($base,['actor'=>$actor,'is_op'=>$requestOp,'has_img'=>$requestImage,'rules'=>[$rule]]));
    }
}
foreach (['','0','0,g','0,','g','a','_ws_','a,_ws_','gg'] as $boards) foreach ([null,'0','g','a'] as $board) foreach ([false,true] as $safe) {
    record_case(array_replace($base,['board'=>$board,'is_sfw'=>$safe,'rules'=>[array_replace($baseRule,['boards'=>$boards])]]));
}
foreach ([0,1] as $reports) foreach ([false,true] as $report) foreach ([0,$now-1,$now,$now+1] as $expiry) {
    record_case(array_replace($base,['is_report'=>$report,'rules'=>[array_replace($baseRule,['report_only'=>$reports,'expires_on'=>$expiry])]]));
}
foreach (['','0','prefix-abc-suffix'] as $signatures) foreach ([null,'','0','abc','ABC','x'] as $browser) foreach ([null,'0','abc','x'] as $signature) {
    record_case(array_replace($base,['browser_id'=>$browser,'req_sig'=>$signature,'rules'=>[array_replace($baseRule,['ua_ids'=>$signatures])]]));
}
foreach ([$ip-1,$ip,$ip+1] as $address) foreach ([0,64512,64513] as $asn) foreach ([0,1] as $active) {
    record_case(array_replace($base,['address'=>$address,'asn'=>$asn,'rules'=>[array_replace($baseRule,['asn'=>64512,'active'=>$active])]]));
}
foreach ([0,$now-2000,$now] as $updated) foreach ([0,1,8] as $pending) foreach ($actors as $actor) {
    if ($actor !== null) $actor['pending']=$pending;
    record_case(array_replace($base,['actor'=>$actor,'rules'=>[array_replace($baseRule,['lenient'=>2,'updated_on'=>$updated])]]));
}
foreach ([0,5,6,9,10,19,20] as $reports) foreach ([0,8] as $pending) foreach ([0,400,1800] as $network) {
    foreach ([500,86400] as $password) foreach ([0,$now-300] as $since) foreach ([1,2] as $lenient) {
        $actor=['password_age'=>$password,'network_age'=>$network,'posts'=>0,'verified'=>0,'score'=>0,'pending'=>$pending,'reports'=>$reports];
        record_case(array_replace($base,['actor'=>$actor,'is_report'=>true,'rules'=>[array_replace($baseRule,['lenient'=>$lenient,'report_only'=>1,'created_on'=>$since])]]));
    }
}
record_case(array_replace($base,['rules'=>[array_replace($baseRule,['id'=>2,'lenient'=>3]),$baseRule]]));
record_case(array_replace($base,['rules'=>[array_replace($baseRule,['id'=>2]),$baseRule]]));
$payload=['files'=>$hashes,'methods'=>$methods,'cases'=>[]];
$json=json_encode($payload,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR);
$rows=array_map(fn($case)=>'        '.json_encode($case,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),$cases);
$json=str_replace('"cases": []','"cases": ['."\n".implode(",\n",$rows)."\n    ]",$json)."\n";
if (count($cases)!==1392 || strlen($json)>1024*1024) { throw new RuntimeException('Unexpected corpus size: '.count($cases)); }
if (in_array('--check',$argv,true)) {
    if (file_get_contents($argv[2])!==$json) { throw new RuntimeException('Network policy fixture differs.'); }
    echo "Network policy fixture matches 1392 cases.\n";
} else { file_put_contents($argv[2],$json); }
