<?php
// Evaluate only the two feed functions against synthetic rows. No original
// request handling, application configuration or database connection is loaded.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-rss-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$source = file_get_contents($root . '/lib/rss.php');
$tokens = token_get_all($source);
foreach (['summarize', 'rss_dump'] as $name) {
    $loaded = false;
    for ($i = 0; $i < count($tokens); ++$i) {
        if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
        $start = $i;
        do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
        if (!is_array($tokens[$i]) || $tokens[$i][1] !== $name) { continue; }
        $depth = 0; $body = ''; $opened = false;
        for ($j = $start; $j < count($tokens); ++$j) {
            $token = $tokens[$j];
            $body .= is_array($token) ? $token[1] : $token;
            if ($token === '{' || (is_array($token) && in_array($token[0], [T_CURLY_OPEN, T_DOLLAR_OPEN_CURLY_BRACES], true))) { ++$depth; $opened = true; }
            if ($token === '}' && --$depth === 0 && $opened) {
                eval($body); $loaded = true; break 2;
            }
        }
    }
    if (!$loaded) { throw new RuntimeException("Missing pure reference function: $name"); }
}
$inputs = [
    '', 'abcdef', 'abcdefg', 'short. Longer first sentence. Later sentence',
    "short<br>First visible sentence<br>Another line", '<br /><br/>Seven!!',
    str_repeat('x', 59), str_repeat('x', 60), str_repeat('x', 61),
    str_repeat('x', 59) . ' boundary next', str_repeat('x', 60) . ' next word',
    str_repeat('x', 58) . ' ' . str_repeat('y', 20) . ' next',
    str_repeat('x', 60) . "\tmore", str_repeat('界', 20) . ' next',
    'https://example.org/only', 'http://example.org/only', '//example.org/only',
    'https://example.org/only a longer sentence remains',
    'HTTPS://example.org/only a longer sentence remains',
    'prefix // tail sentence', 'suffix //',
    '<a href="https://www.4chan.org/faq" target="_blank">https://www.4chan.org/faq</a> another sentence',
    '<a href="//boards.4chan.org/g/" class="quotelink">&gt;&gt;&gt;/g/</a> useful content',
    '<span class="quote">&gt;quoted words</span><br>next sentence',
    '<s>spoiler sentence</s>', '[spoiler]hidden sentence[/spoiler]',
    '<pre class="prettyprint">long code text</pre>',
    'A &amp; B &lt;script&gt;test&lt;/script&gt; are text',
];
$summaries = [];
foreach ($inputs as $input) {
    $summary = summarize($input);
    $summaries[] = ['input' => $input, 'summary' => $summary, 'title' => strip_tags($summary)];
}
// All globals/constants below belong to this synthetic harness.
define('TITLE', '/rss/ - Owned & RSS');
define('SELF_PATH2_ABS', '//boards.example/rss/');
define('DATA_SERVER', '//boards.example/');
define('BOARD_DIR', 'rss');
define('SQLLOG', 'synthetic_rows');
define('RES_DIR2', 'thread/');
define('PHP_EXT2', '');
define('IMG_DIR2', '//images.example/rss/');
define('THUMB_DIR2', '//images.example/rss/');
define('INDEX_DIR', 'synthetic/');
define('FORCED_ANON', 0);
date_default_timezone_set('America/New_York');
setlocale(LC_TIME, 'C');
$rows = [
    ['no' => 23, 'sub' => 'SPOILER<>Owned &amp; subject', 'com' => 'Visible comment', 'name' => 'Named &amp; poster', 'time' => 1730615400, 'tim' => 123, 'ext' => '.png'],
    ['no' => 22, 'sub' => '', 'com' => '[spoiler]hidden sentence[/spoiler]', 'name' => 'Anonymous', 'time' => 1720015200, 'tim' => 122, 'ext' => '.jpg'],
    ['no' => 21, 'sub' => '', 'com' => '<s>Visible spoiler sentence</s>', 'name' => 'Anonymous', 'time' => 1704110400, 'tim' => 121, 'ext' => '.gif'],
    ['no' => 20, 'sub' => '', 'com' => 'short', 'name' => 'Named poster', 'time' => 1704110400, 'tim' => 0, 'ext' => ''],
    // These are saved comment representations, not page callback output.
    ['no' => 19, 'sub' => 'Stored quotes', 'com' => '&gt;&gt;12 &gt;&gt;&gt;/g/34 &gt;&gt;0012 &gt;&gt;&gt;/g/0034 <a href="/g/catalog" class="quotelink">&gt;&gt;&gt;/g/catalog</a>', 'name' => 'Anonymous', 'time' => 1704110400, 'tim' => 0, 'ext' => ''],
];
$row_index = 0;
$queries = [];
function mysql_board_call($query) { global $queries; $queries[] = $query; return true; }
function mysql_fetch_assoc($query) { global $rows, $row_index; return $rows[$row_index++] ?? false; }
function print_page($path, $output) { global $feed; $feed = $output; }
$warnings = [];
set_error_handler(function ($severity, $message) use (&$warnings) { $warnings[] = $message; return true; }, E_ALL);
rss_dump();
restore_error_handler();
$result = [
    'reference' => 'operator-supplied 4chan-old checkout, synthetic inputs only',
    'files' => ['lib/rss.php' => hash('sha256', $source)],
    'environment' => ['php' => PHP_VERSION, 'timezone' => 'America/New_York', 'locale' => 'C'],
    'summaries' => $summaries, 'rows' => $rows, 'feed' => $feed, 'queries' => $queries,
    'warnings' => array_values(array_unique($warnings)),
];
$json = json_encode($result, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) {
        throw new RuntimeException('RSS reference differs from the supplied checkout.');
    }
    echo 'RSS reference matches ' . count($summaries) . " summary cases and the synthetic feed.\n";
} else {
    file_put_contents($argv[2], $json);
}
