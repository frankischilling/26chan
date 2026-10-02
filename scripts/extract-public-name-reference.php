<?php
// Selected pure functions and one fixed name block, with synthetic board policy.
// Never load the application, request handlers, credentials or database code.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-public-name-reference.php SOURCE JSON [--check]\n");
    exit(2);
}
$root = realpath($argv[1]);
$hashes = ['imgboard.php' => 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'lib/postfilter.php' => 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207'];
$policies = [['g', false, false], ['a', false, false], ['b', false, false],
    ['g', true, false], ['jp', false, true]];
if (!$root || !extension_loaded('mbstring') || mb_substitute_character() !== 63) {
    throw new RuntimeException('Pinned source and conversion policy are required.');
}
$sources = [];
foreach ($hashes as $path => $hash) {
    $sources[$path] = file_get_contents($root . '/' . $path);
    if (hash('sha256', $sources[$path]) !== $hash) { throw new RuntimeException('Audited source differs.'); }
}
function selected_functions($source, $names) {
    $tokens = token_get_all($source); $selected = ''; $found = [];
    for ($i = 0; $i < count($tokens); ++$i) {
        if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
        $start = $i;
        do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
        if (!is_array($tokens[$i]) || !in_array($tokens[$i][1], $names, true)) { continue; }
        $name = $tokens[$i][1];
        if (isset($found[$name])) { throw new RuntimeException('Repeated pure body.'); }
        $depth = 0; $opened = false; $body = '';
        for ($j = $start; $j < count($tokens); ++$j) {
            $token = $tokens[$j]; $body .= is_array($token) ? $token[1] : $token;
            if ($token === '{') { ++$depth; $opened = true; }
            if ($token === '}' && --$depth === 0 && $opened) { break; }
        }
        if (!$opened || $depth !== 0 || strlen($body) > 65536) { throw new RuntimeException('Pure body exceeds bounds.'); }
        $selected .= $body . "\n"; $found[$name] = true;
    }
    if (count($found) !== count($names) || strlen($selected) > 131072) {
        throw new RuntimeException('Selected function set differs.');
    }
    return $selected;
}
if (($argv[3] ?? '') === '--worker') {
    $index = (int)($argv[4] ?? '-1');
    if (!isset($policies[$index])) { throw new RuntimeException('Unknown synthetic policy.'); }
    [$board, $code, $sjis] = $policies[$index];
    define('BOARD_DIR', $board); define('CODE_TAGS', $code); define('SJIS_TAGS', $sjis);
    define('STRIP_TRIPCODE', 0); define('S_TOOLONG', 'too_long'); define('S_ANONAME', 'Anonymous');
    define('SALTFILE', 'owned-synthetic-salt-never-opened');
    class OwnedNameStop extends Exception {}
    function error($message, $destination) { throw new OwnedNameStop($message); }
    function get_magic_quotes_gpc() { return false; }
    function file_get_contents_cached($path) { return 'owned-synthetic-source-secure-salt'; }
    eval(selected_functions($sources['lib/postfilter.php'], ['strip_zerowidth', 'strip_emoticons',
        'strip_private_unicode', 'normalize_content', 'convert_to_utf8', 'trans_similar_to_ascii']));
    eval(selected_functions($sources['imgboard.php'], ['sanitize_text']));
    $source = $sources['imgboard.php']; $preprocess = [];
    foreach (preg_split('/\r?\n/', $source) as $line) {
        $line = trim($line);
        if (preg_match('/^\$name\s*=\s*strip_zerowidth\(/', $line)
            || preg_match('/^\$name\s*=\s*strip_emoticons\(/', $line)
            || (str_starts_with($line, '$name = preg_replace(') && str_contains($line, '2000'))
            || (str_starts_with($line, 'if( !strlen( $name )') && str_contains($line, '/^[ |')))
        { $preprocess[] = $line; }
    }
    if (count($preprocess) !== 4) { throw new RuntimeException('Preprocessing statement set differs.'); }
    $begin = strpos($source, '$name  = preg_replace( "/[\\r\\n]/", "", $name );');
    $end = $begin === false ? false : strpos($source, '//Cookies', $begin);
    if ($begin === false || $end === false || $end - $begin > 8192) {
        throw new RuntimeException('Selected name block differs.');
    }
    $body = substr($source, $begin, $end - $begin);
    $function = 'function owned_prepare_name($name) { $dest="synthetic"; '
        . 'if (strlen($name) > 100) error(S_TOOLONG, $dest); '
        . implode("\n", $preprocess)
        . "\n" . '$owned_utf8 = preg_replace("/[\\r\\n]/", "", $name);' . "\n"
        . $body . "\n" . 'if (!strlen($name)) $name = S_ANONAME; return [$name, $owned_utf8]; }';
    eval($function);
    set_error_handler(function ($severity, $message, $file) {
        // PHP 8 warns when the original two-part list has no secure third field.
        // Record only this audited warning; every other warning fails extraction.
        if ($severity === E_WARNING && $message === 'Undefined array key 2'
            && str_starts_with($file, __FILE__ . '(') && str_ends_with($file, "eval()'d code")) {
            ++$GLOBALS['owned_name_warnings']; return true;
        }
        throw new RuntimeException('Unexpected pure source warning.');
    });
    $inputs = ['', 'Anonymous', ' Name ', '  |　| ', '!Admin!!', ' ! ! ', ' A! ', '＃Name﹟!',
        'ｋａｍｉ', '①Name😀', 'A│B', 'A\tB', "A\tB", "A\r\nB", "A\u{200b}B",
        "A\u{00a0}B", "A\u{3000}B", " A\u{f0000} ", "\u{3134f}", "\u{31350}",
        '<owned>&"\'', '&#8238;', '#password', 'Name#password', 'Name#', '###',
        'Name#password###', 'Name##password', 'Name#discarded#password', 'Name###password',
        'Name##password###', 'Name#かみ', 'Name#ｋａｍｉ', 'Name#漢字', 'Name#ééé',
        'Name#¥', 'Name#‾', 'Name#ソソソソソ', 'Name#日本語&"<>', 'Name#①a😀',
        "Name#p\u{200b}ass\r\nword", "Name#pass\u{3164}word", "Name#pa\tssword",
        "Name##pa\tssword", 'Name#＃password', 'Name#p#q#r', 'Name##p#q',
        'Name#A│B', 'Name##é', 'Name##€', 'Name##漢字',
        'Name##&"<>', 'Name#a&#65;', "\u{f8f0}#password", 'moot#4chan',
        str_repeat('n', 100), str_repeat('n', 101), str_repeat('界', 33), str_repeat('界', 34),
        str_repeat('&', 50), str_repeat('&', 51), str_repeat('"', 42), str_repeat('"', 43),
        'A' . str_repeat("\t", 63) . 'B', 'A' . str_repeat("\t", 64) . 'B',
        str_repeat('"', 36) . '#password', str_repeat('"', 37) . '#password'];
    $cases = [];
    foreach ($inputs as $input) {
        $GLOBALS['owned_name_warnings'] = 0;
        try {
            [$html, $utf8] = owned_prepare_name($input);
            $parts = explode('</span> <span class="postertrip">', $html, 2);
            $name_html = $parts[0];
            $name = html_entity_decode($parts[0], ENT_QUOTES | ENT_HTML401, 'UTF-8');
            $trip = $parts[1] ?? null; $modern_trip = $trip;
            if ($trip !== null && str_starts_with($trip, '!!')) {
                $clean = preg_replace('/#+$/', '', $utf8);
                $escaped = htmlspecialchars($clean, ENT_COMPAT | ENT_HTML401, 'UTF-8');
                $secret = explode('#', $escaped, 3)[2];
                $modern_trip = '!!' . substr(base64_encode(hash_hmac('sha256', $secret,
                    str_repeat("\x11", 32), true)), 0, 11);
            }
            $cases[] = compact('input', 'name', 'name_html', 'trip', 'modern_trip') + ['outcome' => 'allow',
                'missing_secure_field_warnings' => $GLOBALS['owned_name_warnings']];
        } catch (OwnedNameStop $error) {
            $cases[] = ['input' => $input, 'outcome' => $error->getMessage(),
                'missing_secure_field_warnings' => $GLOBALS['owned_name_warnings']];
        }
    }
    echo json_encode(compact('board', 'code', 'sjis', 'cases'), JSON_THROW_ON_ERROR);
    exit(0);
}
$groups = [];
foreach (array_keys($policies) as $index) {
    $process = proc_open([PHP_BINARY, '-d', 'memory_limit=128M', __FILE__, $root, $argv[2], '--worker', (string)$index],
        [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes);
    if (!is_resource($process)) { throw new RuntimeException('Synthetic name worker cannot start.'); }
    fclose($pipes[0]); stream_set_blocking($pipes[1], false); stream_set_blocking($pipes[2], false);
    $deadline = microtime(true) + 15; $output = ''; $errors = '';
    do {
        $output .= stream_get_contents($pipes[1], 131073 - strlen($output));
        $errors .= stream_get_contents($pipes[2], 2049 - strlen($errors));
        $status = proc_get_status($process);
        if (strlen($output) > 131072 || strlen($errors) > 2048 || microtime(true) > $deadline) {
            proc_terminate($process, 9); throw new RuntimeException('Synthetic name worker exceeded bounds.');
        }
        if ($status['running']) { usleep(10000); }
    } while ($status['running']);
    $output .= stream_get_contents($pipes[1], 131073 - strlen($output));
    $errors .= stream_get_contents($pipes[2], 2049 - strlen($errors));
    fclose($pipes[1]); fclose($pipes[2]); proc_close($process);
    if ($status['exitcode'] !== 0 || strlen($output) > 131072 || $errors !== '') {
        throw new RuntimeException('Synthetic name worker failed.');
    }
    $groups[] = json_decode($output, true, 32, JSON_THROW_ON_ERROR);
}
$fixture = ['reference' => 'operator-supplied 4chan-old checkout', 'files' => $hashes,
    'extractor_php' => PHP_VERSION, 'extractor_pcre' => PCRE_VERSION,
    'boundary_stubs' => ['fixed synthetic board constants', 'disabled magic quotes', 'terminal error',
        'synthetic source secure-trip salt, never read from disk'],
    'security_replacement' => 'modern_trip uses HMAC-SHA256 with synthetic key 0x11 repeated 32 times and cleaned UTF-8 secret bytes',
    'groups' => $groups];
$json = json_encode($fixture, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (strlen($json) > 262144) { throw new RuntimeException('Name reference exceeds bounds.'); }
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Name reference differs.'); }
} elseif (file_put_contents($argv[2], $json) !== strlen($json)) {
    throw new RuntimeException('Name reference cannot be saved.');
}
echo 'Public name source cases: ' . array_sum(array_map(fn($group) => count($group['cases']), $groups)) . "\n";
