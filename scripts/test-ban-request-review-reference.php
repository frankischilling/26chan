<?php
namespace JanitorReviewOracle;

// Only retained pure branches run. The clock and the history predicate are
// synthetic; no source application, SQL adapter, keys or private lists load.
function time() { return 1700000000; }
date_default_timezone_set('UTC');
$source = json_decode(file_get_contents(__DIR__.'/../fixtures/ban-request-review-source.json'), true, 8, JSON_THROW_ON_ERROR);
if ($source['sha256'] !== 'e1eaa54bd229f0d718d026cb11a757576f67f02a86e21c4bf5ff051bbfbac0d1'
    || strlen(json_encode($source['php'])) > 8192) {
    throw new \RuntimeException('Review fixture provenance differs.');
}
class Oracle {
    private $active;
    public function error($message) { throw new \InvalidArgumentException($message); }
    public function should_ban_global($ip) { return $this->active; }
    public function run($input, $blocks) {
        $this->active = $input['active_local'];
        $request = ['ban_template'=>17, 'warn_req'=>$input['requested_warning'], 'host'=>'192.0.2.1'];
        $_POST = [];
        if ($input['amended']) { $_POST['amend_tpl'] = 18; }
        if ($input['days'] !== null) { $_POST['ban_length'] = $input['days']; }
        if ($input['global_override'] !== null) { $_POST['global'] = $input['global_override'] ? '1' : '0'; }
        $template = [
            'days'=>$input['template_duration'] === 'warning' ? '0' : '7',
            'banlen'=>$input['template_duration'] === 'indefinite' ? 'indefinite' : '',
            'bantype'=>$input['template_global'] ? 'global' : 'local',
            'save_post'=>$input['json_only'] ? 'json_only' : 'everything',
        ];
        try {
            foreach (['amendment', 'duration_scope', 'escalation'] as $name) {
                eval("namespace JanitorReviewOracle;\n".$blocks[$name]);
            }
            $warn_clause = null;
            eval("namespace JanitorReviewOracle;\n".$blocks['duplicates']);
            return [
                'warning_action'=>$warn,
                'seconds'=>$length === '0000-00-00 00:00:00' ? null : strtotime($length) - time(),
                'global'=>$global,
                'duplicates'=>$warn_clause === null ? 'none' : ($warn_clause === '' ? 'all' : 'warnings'),
            ];
        } catch (\InvalidArgumentException $error) {
            return ['error'=>$error->getMessage()];
        }
    }
}
$oracle = new Oracle();
$cases = [];
foreach ([false,true] as $amended)
foreach ([false,true] as $requested)
foreach ([null,-2,-1,0,1,1024,1025] as $days)
foreach ([null,false,true] as $scope)
foreach (['warning','finite','indefinite'] as $duration)
foreach ([false,true] as $global)
foreach ([false,true] as $active)
foreach ([false,true] as $jsonOnly) {
    $input = ['amended'=>$amended, 'requested_warning'=>$requested, 'days'=>$days,
        'global_override'=>$scope, 'template_duration'=>$duration,
        'template_global'=>$global, 'active_local'=>$active, 'json_only'=>$jsonOnly];
    $cases[] = ['input'=>$input, 'expected'=>$oracle->run($input, $source['php'])];
}
$path = __DIR__.'/../crates/domain/tests/fixtures/ban-request-review.json';
$encoded = json_encode($cases, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR)."\n";
if (in_array('--write', $argv, true)) {
    file_put_contents($path, $encoded);
} elseif (file_get_contents($path) !== $encoded) {
    throw new \RuntimeException('Ban-request review decisions differ.');
}
printf("Ban-request review matches %d source decisions.\n", count($cases));
