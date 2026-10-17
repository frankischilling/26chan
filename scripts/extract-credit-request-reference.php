<?php
// Execute only the pinned pure credit-eligibility predicate with synthetic data.
if ($argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
    fwrite(STDERR, "Usage: php extract-credit-request-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
if (PHP_SAPI !== 'cli') { throw new RuntimeException('Only isolated CLI execution is supported.'); }
$sourcePath = realpath($argv[1] . '/lib/postfilter.php');
if (!$sourcePath || !realpath(dirname($argv[2])) || realpath($argv[2]) === $sourcePath) {
    throw new RuntimeException('Use distinct existing source and output paths.');
}
$source = file_get_contents($sourcePath);
if ($source === false || hash('sha256', $source) !== 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207') {
    throw new RuntimeException('Audited classifier source differs.');
}
$start = strpos($source, 'function spam_filter_is_bad_actor()');
$end = strpos($source, 'function spam_filter_get_threat_score', $start);
$body = rtrim(substr($source, $start, $end - $start));
if (hash('sha256', $body) !== '5eb7a455c1fb45f4d3842f79c2b00cfb09d33f4916579ba68c0fb5cd7c0d20ed') {
    throw new RuntimeException('Classifier boundary differs.');
}
error_reporting(E_ERROR | E_PARSE);
$cases = [];
foreach ([null, '', 'en-US'] as $language) {
 foreach ([null, '', 'text/html'] as $accept) {
  foreach ([null, '', 'Mozilla/5.0', 'Android; wv)', 'Android; WV)', 'iPhone', 'iPhone Mobile', 'iphone', "iPhone\0Mobile", '猫; wv)'] as $ua) {
   foreach ([null, '', 'https://sys.4chan.org/g/', 'https://example.test/thread/1', 'https://example.test/THREAD/1'] as $referer) {
    $_SERVER = [];
    foreach (['HTTP_ACCEPT_LANGUAGE'=>$language, 'HTTP_ACCEPT'=>$accept, 'HTTP_USER_AGENT'=>$ua, 'HTTP_REFERER'=>$referer] as $key=>$value) {
      if ($value !== null) { $_SERVER[$key] = $value; }
    }
    // A distinct function name gives each request a fresh source static cache.
    $name = 'owned_credit_case_' . count($cases);
    eval(str_replace('function spam_filter_is_bad_actor()', 'function ' . $name . '()', $body));
    $cases[] = ['language'=>$language, 'accept'=>$accept, 'ua'=>$ua, 'referer'=>$referer, 'withhold'=>$name()];
   }
  }
 }
}
$json = json_encode(['source_sha256'=>hash('sha256',$source), 'function_sha256'=>hash('sha256',$body), 'php_version'=>PHP_VERSION, 'cases'=>$cases], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (($argv[3] ?? '') === '--check') {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Credit predicate fixture differs.'); }
} else { file_put_contents($argv[2], $json); }
fwrite(STDOUT, count($cases) . " isolated source credit-request cases.\n");
