<?php
// Hash-pinned, isolated hint functions. No application or deployment key is run.
if ($argc < 3) { fwrite(STDERR, "Usage: php extract-network-hints-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$source = file_get_contents($argv[1] . '/lib/postfilter.php');
$hash = 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207';
if (hash('sha256', $source) !== $hash) { throw new RuntimeException('Audited source differs.'); }
$bodies = [];
foreach (['spam_filter_get_browser_id', 'spam_filter_get_req_sig'] as $name) {
    $pattern = '/^function ' . preg_quote($name, '/') . '\(\).*?^}/ms';
    if (preg_match_all($pattern, $source, $matches) !== 1 || strlen($matches[0][0]) > 8192) {
        throw new RuntimeException('Selected hint body changed.');
    }
    // Each fixture is a separate request; do not retain the source static cache.
    $body = str_replace('static $cache = null;', '$cache = null;', $matches[0][0], $count);
    if ($count !== 1) { throw new RuntimeException('Hint cache structure changed.'); }
    if ($name === 'spam_filter_get_browser_id') {
        // Remove the entire embedded-key assignment before evaluation. Only the
        // fixed public synthetic fixture key is used or written to the corpus.
        $body = preg_replace('/\$hmac_secret\s*=\s*\'[^\']*\';/',
            '$hmac_secret = hex2bin("' . str_repeat('11', 32) . '");', $body, -1, $count);
        if ($count !== 1) { throw new RuntimeException('Hint key assignment changed.'); }
    }
    $bodies[] = $body;
}
unset($source, $matches, $body);
eval(implode("\n", $bodies));
unset($bodies);
// The source's absent-UA read emits this one known warning before returning its
// falsey-UA marker. Preserve the decision without printing unrelated warnings.
set_error_handler(function($level, $message) {
    if ($level === E_WARNING && $message === 'Undefined array key "HTTP_USER_AGENT"'
        && !isset($_SERVER['HTTP_USER_AGENT'])) { return true; }
    throw new RuntimeException('Unexpected isolated hint warning.');
});
$cases = [];
function record_hint($headers) {
    global $cases;
    $_SERVER = [];
    foreach ($headers as [$name, $value]) {
        $_SERVER['HTTP_' . strtoupper(str_replace('-', '_', $name))] = $value;
    }
    $cases[] = ['headers' => $headers, 'browser_id' => spam_filter_get_browser_id(),
        'request_signature' => spam_filter_get_req_sig()];
}
$agents = [null, '', '0', 'Firefox/123.4', 'Firefox/124.0', 'Desktop:1.2/3..4;X',
    'Slash/:x/./:;/A:0x', 'Android/1', 'android/1', 'iPhone', 'iPad', 'Dalvik',
    'Clover', 'Kuroba', 'ChanOS', 'Mozilla/5.0 (Linux; Android 10; K) Chrome/120.1',
    'Browser/10 日本語', "Browser\tVersion/2"];
foreach ($agents as $ua) foreach ([null, '?0', '?1', '?2'] as $mobile)
foreach ([null, '', '0', '""', '"Pixel 8"'] as $model) foreach ([null, 'navigate', 'cors'] as $mode) {
    $headers = [];
    foreach ([['user-agent',$ua],['sec-ch-ua-mobile',$mobile],['sec-ch-ua-model',$model],['sec-fetch-mode',$mode]] as [$name,$value]) {
        if ($value !== null) { $headers[] = [$name,$value]; }
    }
    record_hint($headers);
}
$required = [['user-agent','Desktop/1'],['accept','*/*'],['referer','https://example.invalid/'],['accept-language','en']];
$orders = [
    [], ['sec-fetch-site','sec-fetch-mode','sec-fetch-dest'],
    ['sec-fetch-dest','sec-fetch-mode','sec-fetch-site'],
    ['sec-fetch-mode','sec-fetch-site','sec-fetch-dest'],
    ['sec-fetch-site','x-ignored','sec-fetch-mode','sec-fetch-dest'],
    ['sec-fetch-site','sec-ch-ua-model','sec-fetch-mode','sec-fetch-dest'],
    ['sec-ch-ua-platform','sec-ch-ua-mobile','sec-ch-ua-model'],
    ['sec-ch-ua-model','sec-ch-ua-platform','sec-ch-ua-mobile'],
    ['SEC-FETCH-SITE','Sec-Fetch-Mode','Sec-Fetch-Dest','sec-fetch-site'],
    ['sec_fetch_site','sec-fetch-mode','sec_fetch_dest'],
    ['sec-fetch-site','sec_fetch_site','sec-fetch-mode','sec-fetch-dest'],
];
foreach ($orders as $order) foreach ([false,true] as $dnt) foreach ([false,true] as $xrw)
foreach ([false,true] as $missing) {
    $headers = $required;
    if ($missing) { array_pop($headers); }
    foreach ($order as $name) { $headers[] = [$name,'']; }
    if ($dnt) { $headers[] = ['sec-gpc','']; }
    if ($xrw) { $headers[] = ['x-requested-with','']; }
    record_hint($headers);
}
$result = ['source_sha256' => $hash, 'synthetic_key_hex' => str_repeat('11', 32),
    'scope' => 'Selected source functions with per-request caches and a replaced synthetic key; no ingress/SAPI ordering claim.',
    'cases' => $cases];
$json = json_encode($result, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR) . "\n";
if (($argv[3] ?? '') === '--check') {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Network hint fixture differs.'); }
} else { file_put_contents($argv[2], $json); }
printf("Network hint fixture matches %d cases.\n", count($cases));
