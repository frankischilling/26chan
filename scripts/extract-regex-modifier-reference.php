<?php
// Synthetic PHP PCRE inputs only; no application, operator data or includes.
if ($argc < 2 || $argc > 3 || ($argc === 3 && $argv[2] !== '--check')) {
    fwrite(STDERR, "Usage: php scripts/extract-regex-modifier-reference.php OUTPUT [--check]\n");
    exit(2);
}
$inputs = [
    ['/^abc$/', "abc\n"], ['/^abc$/D', "abc\n"], ['/^abc$/D', 'abc'],
    ['/abc$/D', "abc\nx"], ['/abc$/mD', "abc\nx"],
    ['/(?m:abc$)/D', "abc\nx"], ['/(?-m:abc$)/mD', "abc\n"],
    ['/abc\Z/D', "abc\n"], ['/abc\$/D', 'abc$'], ['/[$]/D', '$'],
    ['/\Qabc$\E/D', 'abc$'], ['/abc$/DX', "abc\n"],
    ['/\d+/', '123'], ['/\d+/X', '123'], ['/a b/X', 'ab'], ['/a b/xX', 'ab'],
    ['/\q/', 'q'], ['/\q/X', 'q'], ['/|abc$/D', 'abc'], ['/|abc$/D', "abc\n"],
    ["/abc $ # literal dollar follows $\n /xD", "abc\n"],
];
$cases = [];
foreach ($inputs as [$pattern, $text]) {
    $count = @preg_match_all($pattern, $text, $matches);
    $cases[] = ['pattern' => $pattern, 'text' => $text,
        'count' => $count === false ? null : $count];
}
$result = ['version' => 1, 'php' => PHP_VERSION, 'pcre' => PCRE_VERSION, 'cases' => $cases];
if (($argv[2] ?? '') === '--check') {
    $saved = json_decode(file_get_contents($argv[1]), true, flags: JSON_THROW_ON_ERROR);
    // Runtime versions are provenance, not a requirement to replace equal results.
    if ($saved['version'] !== 1 || $saved['cases'] !== $cases) {
        throw new RuntimeException('PHP regexp modifier decisions changed.');
    }
} else {
    file_put_contents($argv[1], json_encode($result,
        JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR) . "\n");
}
fwrite(STDOUT, count($cases) . " PHP regexp modifier cases match.\n");
