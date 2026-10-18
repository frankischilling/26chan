<?php
// Execute only two hash-pinned, audited pure functions with owned synthetic data.
// No application includes, RPC, network, database or session access.
if (PHP_SAPI !== 'cli' || $argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
    throw new RuntimeException('Usage: php -n extract-threat-score-reference.php SOURCE OUTPUT [--check]');
}
if (PHP_VERSION !== '8.4.24' || PCRE_VERSION !== '10.46 2025-08-27' || PHP_INT_SIZE !== 8) {
    throw new RuntimeException('Use the qualified 64-bit PHP 8.4.24 / PCRE 10.46 runtime.');
}
$sourcePath = realpath($argv[1] . '/lib/postfilter.php');
if (!$sourcePath || !realpath(dirname($argv[2])) || realpath($argv[2]) === $sourcePath) {
    throw new RuntimeException('Use distinct existing source and output paths.');
}
$source = file_get_contents($sourcePath);
$sourceHash = 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207';
if (hash('sha256', $source) !== $sourceHash) { throw new RuntimeException('Source differs.'); }
function extract_pinned($source, $name, $hash) {
    $start = strpos($source, 'function ' . $name . '(');
    $end = strpos($source, "\r\n}", $start);
    $body = substr($source, $start, $end + 3 - $start);
    if (hash('sha256', $body) !== $hash) { throw new RuntimeException('Audited function boundary differs: ' . $name); }
    return $body;
}
$scoreHash = '258a972c331faab651eff2f0ee2444553b6ff01ffa5b7238b4fd7920b87c1503';
$languageHash = '31af597ad24fb84c5e19d3b945d83467d986802e3acd78b34fcbbbf9094f41cb';
$scorer = extract_pinned($source, 'spam_filter_get_threat_score', $scoreHash);
$language = extract_pinned($source, 'get_lang_regex_from_country', $languageHash);
define('DEFAULT_BURICHAN', false);
// Missing source array keys are null after a warning. Type errors remain errors.
error_reporting(E_ERROR | E_PARSE);
eval($scorer);
eval($language);
$base = [
 ['HTTP_USER_AGENT','Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120.0 Safari/537.36'],
 ['HTTP_CONTENT_TYPE','multipart/form-data; boundary=----WebKitFormBoundaryabc'],
 ['HTTP_SEC_CH_UA_MOBILE','?0'], ['HTTP_ACCEPT','text/html'],
 ['HTTP_ACCEPT_LANGUAGE','en-US,en;q=0.9'], ['HTTP_REFERER','https://boards.4chan.org/g/'],
 ['HTTP_COOKIE','session=synthetic'], ['HTTP_SEC_FETCH_DEST','empty'],
 ['HTTP_SEC_FETCH_MODE','cors'], ['HTTP_SEC_FETCH_SITE','same-origin']
];
$cases = [];
function record_case($name, $server, $cookies = [['session','synthetic']], $country = null, $op = false, $multipart = true, $challenge = null, $response = null, $time = 1000) {
 global $cases;
 static $names=[];
 if(isset($names[$name])) { throw new RuntimeException('Duplicate synthetic case name.'); }
 $names[$name]=true;
 $_SERVER = [];
 foreach ($server as [$key,$value]) { $_SERVER[$key] = $value; }
 $_SERVER['REQUEST_TIME'] = $time;
 $_COOKIE = [];
 foreach ($cookies as [$key,$value]) { $_COOKIE[$key] = $value; }
 $_POST = [];
 if ($challenge !== null) { $_POST['t-challenge'] = $challenge; }
 if ($response !== null) { $_POST['t-response'] = $response; }
 $before = [$_SERVER,$_COOKIE,$_POST];
 try { $score = spam_filter_get_threat_score($country,$op,$multipart); }
 catch (TypeError $error) { $score = null; }
 if ($before !== [$_SERVER,$_COOKIE,$_POST]) { throw new RuntimeException('Source mutated inputs.'); }
 $cases[] = compact('name','server','cookies','country','op','multipart','challenge','response','time','score');
}
function replace_header($server,$key,$value) {
 $out=[]; $found=false;
 foreach($server as [$k,$v]) {
  if($k===$key) { $found=true; if($value!==null) { $out[]=[$k,$value]; } }
  else { $out[]=[$k,$v]; }
 }
 if(!$found && $value!==null) { $out[]=[$key,$value]; }
 return $out;
}
$uas = ['', '0', 'Mozilla/5.0', 'custom', 'Mozilla/5.0 (Linux) Firefox/51.0', 'Mozilla/5.0 (Linux) Firefox/59.0', 'Mozilla/5.0 (Linux) Firefox/77.0', 'Mozilla/5.0 (Linux) Firefox/111.0', 'Mozilla/5.0 (Linux) Firefox/501.0', 'Mozilla/5.0 (Linux) Firefox/120', 'Mozilla/5.0 (Linux) Firefox/120.0 PaleMoon', 'FxiOS/120.0 iOS', 'iPhone', 'iPhone Mobile', 'Android; wv', 'Dalvik Android', 'iOS', 'Netscape/', 'Opera', 'OperaX', 'compatible; MSIE ', 'HEADLESS python-', 'Mozilla/5.0 (X11; Linux) WebKit Chrome/120.0 Safari/537', 'WebKit Chrome/59.0 Safari/529', 'WebKit Chrome/69.0', 'WebKit Chrome/79.0', 'WebKit Chrome/501.0', 'WebKit Chrome/120', 'Mozilla/5.0 (Windows NT 5.0) Firefox/120.0', 'Mozilla/5.0 (Windows NT 5.0) Goanna Firefox/120.0', 'Mozilla/5.0 (Windows NT 5.0) Mypal/1 Firefox/120.0', 'Mozilla/5.0 (Windows NT 6.0) Firefox/120.0', 'Mozilla/5.0 (Windows NT 11.0) Firefox/120.0', 'Mozilla/5.0 (Mac OS X 9_0) WebKit Safari/600', 'Mozilla/5.0 (Mac OS X 10_6) WebKit Safari/600', 'Mozilla/5.0 (Mac OS X 10_11) WebKit Safari/600', 'Mozilla/5.0 (Mac OS X 11_0) WebKit Safari/600', 'Mozilla/5.0 (Android 3) WebKit Chrome/120.0', 'Mozilla/5.0 (Android 7) WebKit Chrome/120.0', 'Mozilla/5.0 (Android 21; Win64;) WebKit Chrome/120.0', 'like Mac OS iPhone WebKit Safari/600', 'x like Mac OS iPhone WebKit Safari/600'];
$types = ['', 'application/x-www-form-urlencoded', 'multipart/form-data; boundary=----WebKitabc', 'WebKitabc', 'boundary=abc', 'boundary=---123', 'boundary=--GeckoFormBoundarya3', 'boundary=123e4567-e89b-12d3-a456-426614174000'];
foreach($uas as $ui=>$ua) foreach($types as $ti=>$ct) foreach([false,true] as $multipart) {
 $server=replace_header(replace_header($base,'HTTP_USER_AGENT',$ua),'HTTP_CONTENT_TYPE',$ct);
 record_case("ua-$ui-type-$ti-mode-".(int)$multipart,$server,multipart:$multipart);
}
$mutations = [
 'HTTP_ACCEPT'=>[null,'','0','*/*','text/plain','application/signed-exchange'],
 'HTTP_ACCEPT_LANGUAGE'=>[null,'','0','fr','fr-fr','FR-FR','ja;q=1','猫-fr'],
 'HTTP_REFERER'=>[null,'','0','https://example.test/thread/1','sys.4chan.org','4channel.org'],
 'HTTP_SEC_CH_UA'=>[null,'Brave','brave'], 'HTTP_SEC_GPC'=>[null,'','1'],
 'HTTP_SEC_FETCH_USER'=>[null,'','?0','?1'], 'HTTP_PRAGMA'=>[null,'','no-cache'],
 'HTTP_CACHE_CONTROL'=>[null,'','no-cache'], 'HTTP_SEC_CH_UA_PLATFORM'=>[null,'','Windows','Linux','macOS'],
 'HTTP_UPGRADE_INSECURE_REQUESTS'=>[null,'','1'], 'HTTP_VARY'=>[null,'','Origin'],
 'HTTP_PATH'=>[null,'','/'], 'HTTP_SAME_ORIGIN'=>[null,'','true'], 'HTTP_REFERRER_POLICY'=>[null,'','same-origin'],
 'HTTP_SEC_FETCH_MODE'=>[null,'','navigate'], 'HTTP_SEC_FETCH_SITE'=>[null,'','none'],
 'HTTP_SEC_FETCH_DEST'=>[null,'','document'], 'HTTP_SEC_CH_UA_MODEL'=>[null,'','phone'],
 'HTTP_SEC_CH_UA_BITNESS'=>[null,'','64'], 'HTTP_X_HTTP_VERSION'=>[null,'','HTTP/1.1','HTTP/2'],
 'HTTP_COOKIE'=>[null,'','0','cf_clearance=owned'], 'HTTP_X_REQUESTED_WITH'=>[null,'floens','adamantcheese','clover','Clover']
];
foreach($mutations as $key=>$values) foreach($values as $vi=>$value) foreach([0,4,22,41] as $ui) {
 $server=replace_header(replace_header($base,'HTTP_USER_AGENT',$uas[$ui]),$key,$value);
 record_case("header-$key-$vi-ua-$ui",$server);
}
// Exercise cross-term order, cookie fields, loose version comparison and timing.
foreach(['UTC','Etc/GMT','America/New_York','Atlantic/Azores','0',''] as $tz)
 foreach(['33','033',' 33','3.3e1','32','0','', 'junk'] as $version)
 foreach(['0','998','999','1000','1001','junk'] as $time) {
  $server=replace_header($base,'HTTP_X_TIMEZONE','Europe/London');
  record_case("time-".count($cases),$server,[['_tcs',"x.$tz.$time.$version"]],challenge:'owned',response:'answer');
 }
foreach([[],[['cf_clearance','x']],[['_tcs','x']],[['_tcs','x.UTC']],[['_tcs','x.UTC.999.33.extra']]] as $ci=>$cookies) {
 record_case("cookie-$ci",$base,$cookies,challenge:'owned',response:'answer');
 record_case("cookie-noop-$ci",$base,$cookies,challenge:'noop',response:'answer');
}
preg_match_all("/'([A-Z]{2})' =>/",$language,$codes);
foreach(array_unique(array_merge($codes[1],['XX','zz','us','US','0',''])) as $country)
 foreach(['fr','en-US','zz;q=1','ja-JP;q=0.8','x_fr_x','猫fr猫'] as $li=>$lang) {
 record_case("country-$country-$li",replace_header($base,'HTTP_ACCEPT_LANGUAGE',$lang),country:$country);
}
foreach($codes[1] as $country) {
 $pattern=get_lang_regex_from_country($country);
 $alternatives=explode('|',substr($pattern,6,-4));
 foreach($alternatives as $li=>$lang) {
  record_case("language-positive-$country-$li",replace_header($base,'HTTP_ACCEPT_LANGUAGE','x,'.$lang.';q=1'),country:$country);
  record_case("language-boundary-$country-$li",replace_header($base,'HTTP_ACCEPT_LANGUAGE','x'.$lang.'x;q=1'),country:$country);
 }
}
for($gap=0;$gap<8;$gap++) {
 $server=$base;
 $server[]=['HTTP_SEC_CH_UA_MODEL','phone'];
 for($i=0;$i<$gap;$i++) { $server[]=['SERVER_EXTRA_'.$i,'x']; }
 $server[]=['HTTP_SEC_CH_UA_PLATFORM','Windows'];
 record_case("model-gap-$gap",$server);
}
$server=$base;
array_splice($server,9,0,[['SERVER_EXTRA','x']]);
foreach([false,true] as $op) foreach([false,true] as $mp) {
 record_case('sparse-'.(int)$op.'-'.(int)$mp,$server,op:$op,multipart:$mp);
 record_case('navigate-'.(int)$op.'-'.(int)$mp,replace_header($base,'HTTP_SEC_FETCH_MODE','navigate'),op:$op,multipart:$mp);
}
// Deterministic multi-term combinations stress summation and ordered multipliers.
$seed=212;
for($i=0;$i<600;$i++) {
 $server=$base;
 foreach($mutations as $key=>$values) {
  $seed=($seed*1664525+1013904223)&0xffffffff;
  if((($seed>>16)&3)===0) { $server=replace_header($server,$key,$values[($seed>>4)%count($values)]); }
 }
 $server=replace_header($server,'HTTP_USER_AGENT',$uas[$i%count($uas)]);
 record_case("mixed-$i",$server,country:['US','JP','CN',null][$i%4],op:($i%2)===0,multipart:($i%3)!==0);
}
record_case('negative-integer-time-boundary',$base,[['_tcs','x.UTC.-9007199254740993.33']],challenge:'owned',response:'answer',time:-9007199254740992);
record_case('positive-integer-time-boundary',$base,[['_tcs','x.UTC.9007199254740991.33']],challenge:'owned',response:'answer',time:9007199254740992);
record_case('steam-os-without-slash',replace_header($base,'HTTP_USER_AGENT','Mozilla/5.0 (compatible; SteamOS 1.0)'));
foreach([[null,'answer'],['','answer'],['owned',null],['owned',''],['owned','0'],['0','answer'],['noop','answer']] as $i=>[$challenge,$response]) {
 record_case("form-presence-$i",$base,[['_tcs','x.UTC.999.33']],challenge:$challenge,response:$response);
}
$metadata=['source_sha256'=>$sourceHash,'function_sha256'=>$scoreHash,'language_sha256'=>$languageHash,'php_version'=>PHP_VERSION,'pcre_version'=>PCRE_VERSION,'php_int_size'=>PHP_INT_SIZE];
$flags=JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR;
$json=rtrim(json_encode($metadata,JSON_PRETTY_PRINT|$flags));
$json=substr($json,0,-1).",\"cases\":[\n".implode(",\n",array_map(fn($case)=>json_encode($case,$flags),$cases))."\n]}\n";
if(($argv[3]??'')==='--check') {
 if(file_get_contents($argv[2])!==$json) { throw new RuntimeException('Threat fixture differs.'); }
} else { file_put_contents($argv[2],$json); }
echo count($cases)." isolated numeric threat cases.\n";
