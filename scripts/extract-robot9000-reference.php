<?php
// Execute only the pinned plugin's pure reduction block and duration formatter
// on synthetic inputs. Never include its database-backed process function.
if ($argc < 3) { fwrite(STDERR, "Usage: php scripts/extract-robot9000-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$source = file_get_contents($root . '/plugins/robot9000.php');
foreach (['R9K_SIGNAL_RATIO', 'R9K_SNR_MIN_LEN', 'R9K_LOW_SNR', 'R9K_EMPTY_COM', 'R9K_ASCII_ONLY'] as $name) {
    if (!preg_match("/define\\('" . $name . "', [^\\n]+\\);/", $source, $match)) { throw new RuntimeException("Missing constant $name"); }
    eval($match[0]);
}
$start = strpos($source, '$txt = strtolower($com);');
$end = strpos($source, 'if ($mute === false)', $start);
if ($start === false || $end === false || $end - $start > 2500) { throw new RuntimeException('Reduction block is missing or changed.'); }
$block = substr($source, $start, $end - $start);
if (!preg_match('/function r9k_pretty_duration\(\$secs\)\{.*?^\}/ms', $source, $match) || strlen($match[0]) > 2000) {
    throw new RuntimeException('Pure duration function is missing or changed.');
}
eval($match[0]);
$cases = [];
foreach ([
    '', 'café', 'abc', '111abc222', 'aabbbccccdd',
    '123ABC456 <b>AAA</b> &gt;&gt;42 &amp; -- XXxxx!!!',
    'before <a>inside</a> after', 'words >>123 remain', 'words &gt;&gt;123 remain',
    '!!!!!!!!!!!a', '!!!!!!!!!a', 'abcdefghijk', '12345678901',
    '&amp; &unknown; &#039; &#123; &_word; content',
    '&gt;&gt;&gt;/r9k/123 trailing', '<a data-x=">">two</a>',
    'left <unfinished', 'AAA---BBB---', '<b></b>',
    '123abc456789', 'aabbaa---cccc', '<span class="quote">&gt;!!!!!</span>',
    '&gt;<br>&gt;', '<s>&gt;&gt;42</s>', '      a      ',
] as $input) {
    $error = $input === '' ? R9K_EMPTY_COM : (preg_match('/[\x80-\xFF]/', $input) ? R9K_ASCII_ONLY : null);
    if ($error !== null) { $cases[] = ['input' => $input, 'error' => $error]; continue; }
    $com = $input; $mute = false;
    eval($block);
    $cases[] = ['input' => $input, 'normalized' => $stxt, 'reason' => $mute === false ? null : $mute];
}
$durations = [];
foreach ([0, 1, 2, 59, 60, 61, 3600, 86400, 604800, 694861, 31536000] as $seconds) {
    $durations[] = ['seconds' => $seconds, 'text' => r9k_pretty_duration($seconds)];
}
$json = json_encode([
    'reference' => 'operator-supplied 4chan-old checkout',
    'files' => ['plugins/robot9000.php' => hash('sha256', $source), 'imgboard.php' => hash_file('sha256', $root . '/imgboard.php')],
    'normalization' => $cases, 'durations' => $durations,
], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Robot9000 reference differs.'); }
    echo 'Robot9000 reference matches ' . count($cases) . ' reduction cases and ' . count($durations) . " durations.\n";
} else { file_put_contents($argv[2], $json); }
