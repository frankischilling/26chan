<?php
// Execute only four selected hash-pinned functions with synthetic authority.
// Never load the application, private passwords, configuration or a database.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-name-trip-admission-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
$root = realpath($argv[1]);
if (!$root || !extension_loaded('intl') || !extension_loaded('mbstring')) {
    throw new RuntimeException('Reference and ICU/UTF-8 support are required.');
}
$hash = 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207';
$names = ['normalize_ascii', 'strip_zerowidth', 'normalize_text', 'spam_filter_post_trip'];
if (($argv[3] ?? '') === '--worker') {
    $source = file_get_contents($root . '/lib/postfilter.php');
    if (hash('sha256', $source) !== $hash) { throw new RuntimeException('Audited source differs.'); }
    $tokens = token_get_all($source);
    $selected = ''; $found = [];
    for ($i = 0; $i < count($tokens); ++$i) {
        if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
        $start = $i;
        do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
        if (!is_array($tokens[$i]) || !in_array($tokens[$i][1], $names, true)) { continue; }
        $name = $tokens[$i][1];
        if (isset($found[$name])) { throw new RuntimeException('Repeated selected body.'); }
        $depth = 0; $opened = false; $body = '';
        for ($j = $start; $j < count($tokens); ++$j) {
            $token = $tokens[$j]; $body .= is_array($token) ? $token[1] : $token;
            if ($token === '{') { ++$depth; $opened = true; }
            if ($token === '}' && --$depth === 0 && $opened) { break; }
        }
        if (!$opened || $depth !== 0 || strlen($body) > 16384) { throw new RuntimeException('Selected body exceeds bounds.'); }
        $selected .= $body . "\n"; $found[$name] = true;
    }
    if (count($found) !== 4 || strlen($selected) > 32768) { throw new RuntimeException('Selected body set is incomplete.'); }
    define('S_FAILEDUPLOAD', 'upload'); define('S_BANNEDTEXT', 'banned');
    class OwnedStop extends Exception {}
    function error($message, $destination) { throw new OwnedStop($message); }
    function valid() { return $GLOBALS['owned_moderator']; }
    eval($selected);
    $names = [
        'Anonymous', 'moot', 'MOOT', 'mo ot', 'moot Ep8pui8Vw2',
        'mοоt Ep8pui8Vw2', 'moot</span> <span class="postertrip">!Ep8pui8Vw2',
        'moot &lt;Ep8pui8Vw2&gt;', 'ep8pui8vw2', 'ＭＯＯＴＥｐ８ｐｕｉ８Ｖｗ２',
        "moot\u{200b}Ep8pui8Vw2",
    ];
    $trips = [
        '', 'Ep8pui8Vw2', 'ep8pui8vw2', '!Ep8pui8Vw2', 'lollicon', 'LOL1C0M',
        'prefixlolliconsuffix', 'l||1c|n', '||||c||', 'l0l1c0m', 'lollcon',
        'lollicxn', 'lol1c|n', "lollicon\n", 'löllicon', 'quoted&#039;',
    ];
    $cases = [];
    foreach ($names as $name) {
        foreach ($trips as $trip) {
            foreach ([false, true] as $moderator) {
                $GLOBALS['owned_moderator'] = $moderator;
                try { spam_filter_post_trip($name, $trip, 'synthetic'); $outcome = 'allow'; }
                catch (OwnedStop $error) { $outcome = $error->getMessage(); }
                $cases[] = compact('name', 'trip', 'moderator', 'outcome');
            }
        }
    }
    echo json_encode($cases, JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
    exit(0);
}
$process = proc_open([PHP_BINARY, '-d', 'memory_limit=128M', __FILE__, $root, $argv[2], '--worker'],
    [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
if (!is_resource($process)) { throw new RuntimeException('Synthetic worker cannot start.'); }
fclose($pipes[0]); stream_set_blocking($pipes[1], false); stream_set_blocking($pipes[2], false);
$deadline = microtime(true) + 15; $output = ''; $errors = '';
do {
    $output .= stream_get_contents($pipes[1], 262145 - strlen($output));
    $errors .= stream_get_contents($pipes[2], 2049 - strlen($errors));
    $status = proc_get_status($process);
    if (strlen($output) > 262144 || strlen($errors) > 2048 || microtime(true) > $deadline) {
        proc_terminate($process, 9); throw new RuntimeException('Synthetic worker exceeded bounds.');
    }
    if ($status['running']) { usleep(10000); }
} while ($status['running']);
$output .= stream_get_contents($pipes[1], 262145 - strlen($output));
$errors .= stream_get_contents($pipes[2], 2049 - strlen($errors));
fclose($pipes[1]); fclose($pipes[2]); proc_close($process);
if ($status['exitcode'] !== 0 || strlen($output) > 262144 || $errors !== '') {
    throw new RuntimeException('Synthetic worker failed.');
}
$cases = json_decode($output, true, 32, JSON_THROW_ON_ERROR);
if (count($cases) !== 352) { throw new RuntimeException('Source case set differs.'); }
$json = json_encode(['reference' => 'operator-supplied 4chan-old checkout', 'functions' => $names,
    'files' => ['lib/postfilter.php' => $hash], 'extractor_php' => PHP_VERSION,
    'extractor_pcre' => PCRE_VERSION, 'extractor_icu' => INTL_ICU_VERSION,
    'boundary_stubs' => ['moderator decision', 'terminal error'], 'cases' => $cases],
    JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (strlen($json) > 262144) { throw new RuntimeException('Fixture exceeds bounds.'); }
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Name/trip reference differs.'); }
} elseif (file_put_contents($argv[2], $json) !== strlen($json)) {
    throw new RuntimeException('Fixture cannot be saved.');
}
echo 'Name/trip source cases: ' . count($cases) . "\n";
