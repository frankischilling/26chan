<?php
// Execute exactly one audited, hash-pinned pure function with synthetic inputs.
// Never require/include the application, config, session, RPC, or database code.
// Usage: php -n scripts/extract-report-threat-reference.php SOURCE INPUT OUTPUT [--check]
if ($argc < 4 || $argc > 5 || ($argc === 5 && $argv[4] !== '--check')) {
    fwrite(STDERR, "Usage: php -n scripts/extract-report-threat-reference.php SOURCE INPUT OUTPUT [--check]\n");
    exit(2);
}
if (PHP_SAPI !== 'cli') { throw new RuntimeException('Only isolated CLI execution is supported.'); }
if (!function_exists('token_get_all')) {
    throw new RuntimeException('The audited extractor requires tokenizer; with php -n load only the official tokenizer.so explicitly.');
}
$inputPath = realpath($argv[2]);
$outputDirectory = realpath(dirname($argv[3]));
if (!$inputPath || !$outputDirectory || $inputPath === $outputDirectory . '/' . basename($argv[3])) {
    throw new RuntimeException('Use distinct existing-input and generated-output paths.');
}
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$sourceHash = 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207';
$bodyHash = '258a972c331faab651eff2f0ee2444553b6ff01ffa5b7238b4fd7920b87c1503';
$name = 'spam_filter_get_threat_score';
$source = file_get_contents($root . '/lib/postfilter.php');
if ($source === false || hash('sha256', $source) !== $sourceHash) {
    throw new RuntimeException('Audited threat source differs.');
}
$tokens = token_get_all($source);
$selected = null;
$startLine = null;
for ($i = 0; $i < count($tokens); ++$i) {
    if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
    $start = $i;
    do { ++$i; } while (isset($tokens[$i]) && is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
    if (!isset($tokens[$i]) || !is_array($tokens[$i]) || $tokens[$i][1] !== $name) { continue; }
    if ($selected !== null) { throw new RuntimeException('Repeated named source function.'); }
    $startLine = $tokens[$start][2];
    $depth = 0;
    $opened = false;
    $body = '';
    for ($j = $start; $j < count($tokens); ++$j) {
        $token = $tokens[$j];
        $body .= is_array($token) ? $token[1] : $token;
        // Literal/comment braces are inside token arrays and never delimit code.
        if ($token === '{') { ++$depth; $opened = true; }
        if ($token === '}') {
            --$depth;
            if ($opened && $depth === 0) { break; }
        }
    }
    if (!$opened || $depth !== 0 || strlen($body) > 32768) {
        throw new RuntimeException('Selected source function exceeds extraction bounds.');
    }
    $selected = $body;
}
if ($selected === null || hash('sha256', $selected) !== $bodyHash || $startLine !== 1684
    || $startLine + substr_count($selected, "\n") !== 2316) {
    throw new RuntimeException('Exact source-function boundary differs.');
}

// This is an explicit allowlist, not a claim that arbitrary PHP is sandboxed.
// The full-file/body hashes above are the authority for the reviewed source.
$allowedNames = [$name, 'DEFAULT_BURICHAN', 'null', 'false', 'true', 'preg_match',
    'strpos', 'array_keys', 'strlen', 'count', 'explode', 'array_search', 'abs',
    'in_array', 'array_sum', 'round', 'get_lang_regex_from_country'];
$forbidden = [T_INCLUDE, T_INCLUDE_ONCE, T_REQUIRE, T_REQUIRE_ONCE, T_EVAL,
    T_EXIT, T_GLOBAL, T_NEW, T_OBJECT_OPERATOR, T_DOUBLE_COLON, T_VARIABLE];
$functionCount = 0;
foreach (token_get_all('<?php ' . $selected) as $token) {
    if (!is_array($token)) {
        if ($token === '`') { throw new RuntimeException('Shell execution is forbidden.'); }
        continue;
    }
    [$type, $text] = $token;
    if ($type === T_FUNCTION) { ++$functionCount; }
    if ($type === T_STRING && !in_array($text, $allowedNames, true)) {
        throw new RuntimeException('Unreviewed source identifier: ' . $text);
    }
    if ($type === T_VARIABLE) {
        if (in_array($text, ['$GLOBALS', '$_GET', '$_REQUEST', '$_FILES', '$_SESSION', '$_ENV'], true)) {
            throw new RuntimeException('Unexpected global input in source function.');
        }
        continue;
    }
    if (in_array($type, $forbidden, true)) {
        throw new RuntimeException('External-effect syntax is forbidden in the source function.');
    }
}
if ($functionCount !== 1) { throw new RuntimeException('Only one source function may execute.'); }
// The source reads this otherwise unused constant when choosing a domain.
define('DEFAULT_BURICHAN', false);
// Country is always NULL for reporting. Never load the source language helper.
function get_lang_regex_from_country($country) {
    throw new RuntimeException('Country helper must remain unreachable for report arguments.');
}
set_error_handler(function ($severity, $message, $file, $line) {
    throw new ErrorException($message, 0, $severity, $file, $line);
});
eval($selected);

$inputRaw = file_get_contents($argv[2]);
if ($inputRaw === false || strlen($inputRaw) > 1048576) {
    throw new RuntimeException('Input fixture is missing or exceeds bounds.');
}
$input = json_decode($inputRaw, true, 64, JSON_THROW_ON_ERROR);
if (($input['files']['lib/postfilter.php'] ?? null) !== $sourceHash
    || ($input['function_sha256'] ?? null) !== $bodyHash
    || ($input['call_args'] ?? null) !== [null, true, false]
    || ($input['threshold'] ?? null) !== 0.4
    || !isset($input['cases']) || !is_array($input['cases']) || count($input['cases']) > 256) {
    throw new RuntimeException('Input fixture does not match the audited report contract.');
}
$cases = [];
$names = [];
foreach ($input['cases'] as $case) {
    if (!is_string($case['name'] ?? null) || isset($names[$case['name']])
        || !is_bool($case['expected_signal'] ?? null)
        || !is_array($case['headers'] ?? null) || count($case['headers']) > 64
        || !is_array($case['cookies'] ?? null)) {
        throw new RuntimeException('Invalid or duplicate synthetic case.');
    }
    $names[$case['name']] = true;
    $_SERVER = [];
    $_COOKIE = $case['cookies'];
    $_POST = [];
    foreach ($case['headers'] as $header) {
        if (!is_string($header['name'] ?? null) || !preg_match('/^[a-z0-9-]{1,64}$/D', $header['name'])
            || !is_string($header['value'] ?? null) || strlen($header['value']) > 4096
            || preg_match('/[\r\n\x00]/', $header['value'])) {
            throw new RuntimeException('Invalid synthetic HTTP header.');
        }
        $key = 'HTTP_' . strtoupper(str_replace('-', '_', $header['name']));
        if (array_key_exists($key, $_SERVER)) { throw new RuntimeException('Duplicate synthetic server header.'); }
        $_SERVER[$key] = $header['value'];
    }
    // No source time/country/session/database facilities are exercised.
    if (isset($_COOKIE['_tcs'])) { throw new RuntimeException('Time-cookie branch is outside this fixture.'); }
    foreach (['HTTP_USER_AGENT', 'HTTP_CONTENT_TYPE', 'HTTP_SEC_CH_UA_MOBILE'] as $key) {
        if (!array_key_exists($key, $_SERVER)) { throw new RuntimeException('Required warning-free synthetic input missing.'); }
    }
    $serverBefore = $_SERVER;
    $cookieBefore = $_COOKIE;
    $score = spam_filter_get_threat_score(null, true, false);
    if ($_SERVER !== $serverBefore || $_COOKIE !== $cookieBefore || $_POST !== []) {
        throw new RuntimeException('Selected function unexpectedly mutated synthetic inputs.');
    }
    if (!is_float($score) || !is_finite($score) || $score < 0.0) {
        throw new RuntimeException('Source returned an invalid threat score.');
    }
    if ($case['expected_signal'] && $score < 0.4) {
        throw new RuntimeException('Positive bounded proof is not supported by full source: ' . $case['name']);
    }
    // A false signal deliberately makes NO claim that the full score is low.
    $case['source_score'] = $score;
    $case['source_at_or_above_threshold'] = $score >= 0.4;
    $cases[] = $case;
}
$output = $input;
$output['evidence'] = 'Executed exact isolated source function against synthetic inputs; positive signal implies score >= 0.4, negative signal is not a low-score proof.';
$output['extractor_php'] = PHP_VERSION;
$output['extractor_pcre'] = PCRE_VERSION;
$output['cases'] = $cases;
$json = json_encode($output, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (strlen($json) > 1048576) { throw new RuntimeException('Output fixture exceeds bounds.'); }
if ($argc === 5) {
    if (file_get_contents($argv[3]) !== $json) { throw new RuntimeException('Scored threat reference differs.'); }
} elseif (file_put_contents($argv[3], $json) !== strlen($json)) {
    throw new RuntimeException('Scored threat reference could not be saved.');
}
echo 'Isolated report-threat source cases: ' . count($cases) . "\n";
