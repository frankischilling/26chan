<?php
// Execute only the retained caller predicate with synthetic metadata and a
// recording stub. No application, deployment keys or private lists are loaded.
$fixture = json_decode(file_get_contents(__DIR__.'/../fixtures/template-range-caller-reference.json'), true, 8, JSON_THROW_ON_ERROR);
if ($fixture['sha256'] !== '4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb'
    || strlen($fixture['php']) > 1024) {
    throw new RuntimeException('Caller fixture provenance differs.');
}
function process_auto_rangeban(...$args) { $GLOBALS['calls'][] = $args; }
$cases = 0;
foreach ([false, true] as $warn) {
    foreach ([null, ['is_mobile'=>false], ['is_mobile'=>true,'browser_id'=>'1deadbeef']] as $_post_meta) {
        foreach ([null, '', 'g1', 'Global1', 'global1', 'prefixglobal2', '0'] as $rule) {
            $template = $rule === null ? null : ['rule'=>$rule, 'no'=>17];
            $host='192.0.2.1'; $row=['resto'=>123, 'no'=>456]; $GLOBALS['calls']=[];
            eval($fixture['php']);
            $expected = !$warn && $_post_meta !== null && $_post_meta['is_mobile']
                && in_array($rule, ['global1','prefixglobal2'], true);
            if ($GLOBALS['calls'] !== ($expected ? [[$host,'1deadbeef',123,456,17,1]] : [])) {
                throw new RuntimeException('Template range caller differs at case '.$cases);
            }
            ++$cases;
        }
    }
}
printf("Template range caller matches %d cases.\n", $cases);
