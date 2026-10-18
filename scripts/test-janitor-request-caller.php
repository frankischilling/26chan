<?php
namespace OwnedJanitorCaller;

function cases() {
    $cases = [];
    foreach ([false,true] as $warning)
    foreach ([false,true] as $async)
    foreach ([false,true] as $canWarn)
    foreach ([false,true] as $templateWarning)
    foreach (['1deadbeef:synthetic:1:0','0deadbeef:synthetic:1:0',''] as $metadata)
    foreach (['global1','local1'] as $rule) {
        $cases[] = ['warning'=>$warning,'async'=>$async,'can_warn'=>$canWarn,'template_warning'=>$templateWarning,'metadata'=>$metadata,'rule'=>$rule,'parent'=>0];
    }
    $cases[] = ['warning'=>false,'async'=>false,'can_warn'=>true,'template_warning'=>false,'metadata'=>'1deadbeef:synthetic:1:0','rule'=>'global1','parent'=>99];
    return $cases;
}

if (!in_array('--child', $argv, true)) {
    $run = function($index, $mutation = false) {
        $command = [PHP_BINARY, '-n', __FILE__, '--child', (string)$index];
        if ($mutation) $command[] = '--mutate-target';
        $process = proc_open($command, [0=>['pipe','r'],1=>['pipe','w'],2=>['pipe','w']], $pipes);
        if (!is_resource($process)) throw new \RuntimeException('Cannot start isolated caller');
        fclose($pipes[0]);
        $output = stream_get_contents($pipes[1]); fclose($pipes[1]);
        $errors = stream_get_contents($pipes[2]); fclose($pipes[2]);
        if (proc_close($process) !== 0 || $errors !== '' || strlen($output)>65536) {
            throw new \RuntimeException('Isolated caller failed: '.$errors);
        }
        return json_decode($output, true, 16, JSON_THROW_ON_ERROR);
    };
    $results = [];
    foreach (cases() as $index=>$input) $results[] = ['input'=>$input,'expected'=>$run($index)];
    // A control that changes the source target variable must activate a mobile
    // global event. The unchanged caller must never be mistaken for that fix.
    $index = count($results)-1;
    if ($results[$index]['expected']['range_calls'] !== []
        || $results[$index]['expected']['metadata_mobile'] !== true
        || $results[$index]['expected']['requests'] !== 1
        || count($run($index, true)['range_calls']) !== 1) {
        throw new \RuntimeException('Janitor caller target-variable control failed');
    }
    $path = __DIR__.'/../crates/domain/tests/fixtures/janitor-request-caller.json';
    $encoded = json_encode($results, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR)."\n";
    if (in_array('--write', $argv, true)) file_put_contents($path, $encoded);
    elseif (file_get_contents($path) !== $encoded) throw new \RuntimeException('Janitor request caller decisions differ');
    printf("Janitor request caller matches %d cases and its target-variable control.\n", count($results));
    exit;
}

$input = cases()[(int)$argv[2]];
$fixture = json_decode(\file_get_contents(__DIR__.'/../fixtures/janitor-request-caller-source.json'), true, 8, JSON_THROW_ON_ERROR);
if ($fixture['adminBanReq']['sha256'] !== '4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb'
    || $fixture['decode_user_meta']['sha256'] !== '571d154aa43cd51ee17bb4dfec5502d6dd63c265f4a2526820c011c934c7d185') {
    throw new \RuntimeException('Janitor caller provenance differs');
}
$body = $fixture['adminBanReq']['php'];
if (in_array('--mutate-target', $argv, true)) {
    $body = str_replace("decode_user_meta(\$row['email'])", "decode_user_meta(\$post['email'])", $body, $changed);
    if ($changed !== 1) throw new \RuntimeException('Expected one target-variable control');
}
eval('namespace OwnedJanitorCaller;'.$fixture['decode_user_meta']['php']);
eval('namespace OwnedJanitorCaller;'.$body);
define('BOARD_DIR', 'g');
$_GET = ['id'=>7];
$_POST = ['template'=>17,'no'=>7];
if ($input['warning']) $_POST['warn_req'] = '1';
if ($input['async']) $_POST['by_tpl'] = 17;
$_COOKIE = ['4chan_auser'=>'Owned janitor'];
$GLOBALS['trace'] = ['requests'=>0,'delete_calls'=>[],'range_calls'=>[],
    'warning_request'=>null,'error'=>null,'undefined_row'=>false,
    'metadata_mobile'=>(bool)(decode_user_meta($input['metadata'])['is_mobile'] ?? false)];

function mysql_board_call($query, ...$args) {
    global $input;
    if ($query === 'SELECT sub FROM `%s` WHERE no = %d') return ['sub'=>'Owned thread'];
    if ($query !== 'SELECT * FROM `%s` WHERE no=%d') throw new \RuntimeException('Unexpected board query');
    return ['no'=>7,'host'=>'192.0.2.1','archived'=>false,'ext'=>'','file_deleted'=>false,
        'pwd'=>'synthetic','resto'=>$input['parent'],'md5'=>'','name'=>'Owned post','email'=>$input['metadata']];
}
function mysql_num_rows($result) { return 1; }
function mysql_fetch_assoc($result) { return $result; }
function access_board($board) { return true; }
function has_level($level) { return true; }
function gethostbyaddr($address) { return 'owned.invalid'; }
function mysql_global_call($query, ...$args) {
    global $input;
    if ($query === 'SELECT * FROM ban_templates WHERE no=%d') return [
        'no'=>17,'level'=>'janitor','publicreason'=>'Owned','can_warn'=>$input['can_warn'],
        'days'=>$input['template_warning'] ? '0' : '3','save_post'=>'none','name'=>'Owned','bantype'=>'global','rule'=>$input['rule']];
    if (str_starts_with($query, 'SELECT xff FROM xff ')) return ['192.0.2.2'];
    if (str_starts_with($query, 'SELECT COUNT(*) FROM reports ')) return [0];
    if (str_starts_with($query, 'INSERT INTO ban_requests SET ')) {
        $GLOBALS['trace']['requests']++;
        $GLOBALS['trace']['warning_request'] = (bool)$args[count($args)-1];
        return true;
    }
    throw new \RuntimeException('Unexpected global query');
}
function mysql_fetch_row($result) { return $result; }
function mysql_result($result, $row, $column) { return $result[0]; }
function delete_post(...$args) { $GLOBALS['trace']['delete_calls'][]=$args; }
function json_for_post(...$args) { return '{}'; }
function process_auto_rangeban(...$args) { $GLOBALS['trace']['range_calls'][]=$args; }
function error($message) { throw new \InvalidArgumentException($message); }
set_error_handler(function($level, $message) {
    if (str_contains($message, 'Undefined variable $row')) $GLOBALS['trace']['undefined_row']=true;
    elseif (!str_contains($message, 'Trying to access array offset on')) throw new \RuntimeException($message);
    return true;
});
ob_start();
register_shutdown_function(function() {
    ob_end_clean();
    echo json_encode($GLOBALS['trace'], JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR),"\n";
});
try { adminBanReq(); }
catch (\InvalidArgumentException $error) { $GLOBALS['trace']['error']=$error->getMessage(); }
