<?php
// Execute only the pinned pure automation predicate with owned cache facts.
if ($argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
    fwrite(STDERR, "Usage: php extract-automation-request-reference.php SOURCE OUTPUT [--check]\n");
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
$start = strpos($source, 'function spam_filter_is_likely_automated(');
$end = strpos($source, 'function spam_filter_is_pwd_blocked(', $start);
$body = rtrim(substr($source, $start, $end - $start));
if (hash('sha256', $body) !== '96e82ef37f2a3f2903cd20f2eb2065dfd704792336e51640f25a6a82deb378b7') {
    throw new RuntimeException('Classifier boundary differs.');
}
error_reporting(E_ERROR | E_PARSE);
class OwnedMarker {
    public function __construct(private bool $marker) {}
    public function get($key) {
        if ($key !== 'bmbot192.0.2.1') { throw new RuntimeException('Unexpected cache key'); }
        return $this->marker;
    }
}
eval($body);
$cases = [];
foreach ([null, -1, 0, 1, 2, 28, 29, 30, 80, 100, 2147483647] as $score) {
 foreach ([null, '', 'Mozilla/5.0', 'Android; wv)', 'Android; WV)', 'iPhone', 'iPhone Mobile', 'iPhone Safari', 'iPad', 'iPad Mobile', 'ipad', "iPhone\0Mobile", '猫; wv)'] as $ua) {
  foreach ([null, false, true] as $cached) {
   $_SERVER = ['REMOTE_ADDR'=>'192.0.2.1'];
   if ($score !== null) { $_SERVER['HTTP_X_BOT_SCORE'] = (string)$score; }
   if ($ua !== null) { $_SERVER['HTTP_USER_AGENT'] = $ua; }
   $cache = $cached === null ? null : new OwnedMarker($cached);
   $cases[] = ['score'=>$score, 'ua'=>$ua, 'cached'=>$cached,
               'automated'=>spam_filter_is_likely_automated($cache)];
  }
 }
}
$json = json_encode(['source_sha256'=>hash('sha256',$source), 'function_sha256'=>hash('sha256',$body), 'php_version'=>PHP_VERSION, 'cases'=>$cases], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (($argv[3] ?? '') === '--check') {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Automation predicate fixture differs.'); }
} else { file_put_contents($argv[2], $json); }
fwrite(STDOUT, count($cases) . " source automation-request cases.\n");
