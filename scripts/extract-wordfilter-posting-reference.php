<?php
// Audited pure functions only, on synthetic comments in separate workers.
if ($argc < 3) { fwrite(STDERR, "Usage: php scripts/extract-wordfilter-posting-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$hashes = [
    'global' => 'f5ff2ac132b86fa4889e4312f3c2162ab121854cffeb33505f22bfbd2c65b439',
    'ck' => '18f751bc00a43986ce8b73b32c885240b600959abdd993ad2f9ac53fcf29d952',
    'asp' => '90999f8aebb36bfb516ce3da3d41c7fd1bfff142273ec90f5bf10af46505c5f0',
    'v' => '5801c47c10383f99eb5826b8625c3d4cc3d808ec4ced74a6d2c3ae3f1eae13d6',
    'test' => '6eebf9ea022c07ae445097fee1be1951fc723dbf03f51ac62df790eee48eb6d5',
];
$app = file_get_contents($root . '/imgboard.php');
$util = file_get_contents($root . '/lib/util.php');
$postfilter = file_get_contents($root . '/lib/postfilter.php');
if (hash('sha256', $app) !== 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445') {
    throw new RuntimeException('Audited posting source differs.');
}
if (hash('sha256', $util) !== '5307cea255e2d3eb243c7549e1a6521281bf6253c47e010476412b9a4c63a70c') {
    throw new RuntimeException('Audited UTF-8 source differs.');
}
if (hash('sha256', $postfilter) !== 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207') {
    throw new RuntimeException('Audited comment sanitation source differs.');
}
function load_owned_pure_function($source, $name) {
    $tokens = token_get_all(preg_replace('/^<\?(?=\s)/', '<?php', $source));
    for ($i = 0; $i < count($tokens); ++$i) {
        if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
        $start = $i;
        do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
        if (!is_array($tokens[$i]) || $tokens[$i][1] !== $name) { continue; }
        $depth = 0; $body = ''; $opened = false;
        for ($j = $start; $j < count($tokens); ++$j) {
            $token = $tokens[$j]; $body .= is_array($token) ? $token[1] : $token;
            if ($token === '{') { ++$depth; $opened = true; }
            if ($token === '}' && --$depth === 0 && $opened) { eval($body); return; }
        }
    }
    throw new RuntimeException('Pure function is missing: ' . $name);
}
if (($argv[3] ?? '') === '--worker') {
    $profile = $argv[4] ?? '';
    if (!isset($hashes[$profile])) { throw new RuntimeException('Unknown pure filter.'); }
    $filter = file_get_contents($root . '/wordfilters/' . $profile . '.php');
    if (hash('sha256', $filter) !== $hashes[$profile]) { throw new RuntimeException('Audited filter differs.'); }
    foreach ([1, 2] as $number) {
        if ($profile !== 'test') { break; }
        $draw = '$roll' . $number . ' = mt_rand(0, 5);';
        if (substr_count($filter, $draw) !== 1) { throw new RuntimeException('Random draw changed.'); }
        $filter = str_replace($draw, '$roll' . $number . ' = $GLOBALS["owned_filter_rolls"][' . ($number - 1) . '];', $filter);
    }
    eval(substr($filter, strlen('<?php')));
    foreach (['find_match_and_prefix', 'parse_bbcode_one', 'sjis_parse', 'spoiler_parse', 'code_parse', 'parse_op_markup',
        'normalize_link_cb', 'normalize_links', 'boards_matching_arr', 'normalize_and_linkify', 'clean_internal_link',
        'auto_link_static_cb', 'wordwrap2', 'truncate_comment', 'sanitize_text'] as $function) {
        load_owned_pure_function($app, $function);
    }
    load_owned_pure_function($util, 'utf8_wordwrap');
    foreach (['strip_emoticons', 'strip_private_unicode'] as $function) {
        load_owned_pure_function($postfilter, $function);
    }
    if (!preg_match('/\$valid_boards\s*=\s*"([a-z0-9|]+)";/', $app, $match)) { throw new RuntimeException('Board allowlist missing.'); }
    $valid_boards = $match[1];
    define('BOARD_DIR', 'g'); define('SJIS_TAGS', true); define('CODE_TAGS', true);
    // Ordinary public comments cannot take the legacy privileged HTML branch.
    $html = 0;
    // Removed from PHP 8; the audited deployment did not enable magic quotes.
    if (!function_exists('get_magic_quotes_gpc')) { function get_magic_quotes_gpc() { return false; } }
    class L { static function d($board) { return '4chan.org'; } }
    mb_internal_encoding('UTF-8');
    ini_set('default_charset', 'UTF-8');
    $inputs = [
        'ordinary text fam soy CUCK finna pcfat',
        '[code]soy fam CUCK[/code]',
        "t\ntext [code]textttt[/code]",
        '[spoiler]soy fam[/spoiler] [b]finna soy[/b]',
        '[sjis]soy fam[/sjis] [red]soy CUCK[/red] [i]text[/i]',
        '[code]texttt[/code] [code]textttt[/code]',
        '<script>& " &#x3C; [spoiler]ordinary text[/spoiler]',
        '&&&&&&""""""',
        str_repeat('x', 35) . ' & "',
        str_repeat('&', 7) . 'short',
        str_repeat('界', 35) . ' & "',
        'https://www.4chan.org/faq?x=1&y=2 soy',
        'https://boards.4chan.org/g/thread/42 & "',
        ">soy\n>>42\n >>>/g/catalog\n>fam https://www.4chan.org/faq",
        'prefix' . str_repeat('x', 34) . '>>1234567890 [b]' . str_repeat('界', 36) . '[/b]',
        '[sjis]text [b]soy[/b] other[/sjis]',
        '[b]soy[red]fam[/b] CUCK[/red] tail',
        '[spoiler][spoiler]soy[/spoiler][/spoiler]',
        '&4mp; &qu0t; &qu07; ordinary',
        "[code]\nsoy\nfam\n[/code] tail",
        'fa~?rep?~m so~?erep?~y CU~?rep?~CK',
        '[co~?rep?~de]soy fam CUCK[/code]',
        '~?re~?rep?~p?~fam',
        'https://www.4chan.org/faq/~?re~?rep?~p?~owned',
        " \tsoy\u{00a0} fam\u{00ad} CUCK\r\n>>>/g/42 [code]text[/code]\t ",
        "\u{F0000} soy \u{F0000}",
        "\u{1F600}fa\u{1F600}m [spoiler]soy[/spoiler]",
        'fa[spoiler]m[/spoiler]x CUCK',
        "s\u{1c82}y \u{1c89}soy soy\u{1c89}\u{1c89} \u{1ccf0}soy",
        '{{w_br}} ordinary text',
        str_repeat('x', 32) . '{{w_br}}',
        '~?rep?~>soy~?erep?~',
        'https://www.4chan.org/faq/~?rep?~owned~?erep?~',
        'https://boards.4chan.org/g/thread/42#p43 {{w_br}} & "',
        str_repeat('x ', 148) . 'x&"tail',
        str_repeat('x ', 149) . '"tail',
        str_repeat('x ', 148) . 'x\'tail',
    ];
    $choices = [[null, null]];
    if ($profile === 'test') {
        $choices = [];
        for ($first = 0; $first < 6; ++$first) { for ($second = 0; $second < 6; ++$second) { $choices[] = [$first, $second]; } }
    }
    $cases = [];
    foreach ($inputs as $input) {
        // imgboard.php:5293-5390 and 5712-5715, with the fixed SJIS/code
        // policy above. Execute sanitation before the built-in filter path.
        $comment = str_replace(["\r\n", "\r"], "\n", $input);
        $comment = str_replace(["\xC2\xAD", "\xC2\xA0"], '', $comment);
        $comment = strip_emoticons($comment, SJIS_TAGS);
        if (!strlen($comment) || preg_match('/^[ |　|\t]*$/', $comment)) { $comment = ''; }
        if (stripos($comment, '[spoiler]') !== false) {
            $comment = preg_replace('/(\S)\[spoiler\](.*?)\[\/spoiler\](\S)/', '\\1\\2\\3', $comment);
        }
        $comment = preg_replace('#>>>/' . BOARD_DIR . '/([0-9]+)#', '>>$1', $comment);
        $comment = strip_private_unicode(sanitize_text($comment, 1, true));
        if (strpos($comment, 'rep?~') !== false) {
            $comment = str_replace(['~?rep?~', '~?erep?~'], ['', ''], $comment);
        }
        // Decode only the sanitizer's own escaping for the Rust raw-text
        // admission comparison; rendering continues from the escaped value.
        $admission_input = htmlspecialchars_decode($comment, ENT_QUOTES);
        $comment = str_replace("\n", '', nl2br($comment, false));
        $comment = sjis_parse($comment);
        $comment = spoiler_parse($comment);
        if (stripos($comment, '<s>') !== false) { $comment = preg_replace('/<s>(\s|<br>|(?R))*<\/s>/', '', $comment); }
        $comment = preg_replace('#(\[code\](.{0,6})\[\/code\])#', '\\2', $comment);
        $comment = code_parse($comment);
        $comment = str_replace('<pre class="prettyprint"><br>', '<pre class="prettyprint">', $comment);
        $comment = preg_replace('#(<br>){4,}#', '<br><br><br>', $comment);
        $comment = parse_op_markup($comment);
        foreach ($choices as $rolls) {
            $GLOBALS['owned_filter_rolls'] = $rolls;
            $filtered = word_filter($comment, 'com');
            $linked = normalize_and_linkify($filtered);
            $wrapped = wordwrap2($linked, 35, '{{w_br}}');
            $final = preg_replace('#(&gt;&gt;&gt;/[a-z0-9]+/[^ <$]*|&gt;&gt;[0-9]+)#', '~?rep?~\\1~?erep?~', $wrapped);
            $final = preg_replace('!(^|r>|r> )(&gt;[^<]*)!', '\\1<span class="quote">\\2</span>', $final);
            $final = preg_replace('#~?rep?~<span class="quote">(.+?)</span>~?erep?~#', '\\1', $final);
            $final = str_replace(['~?rep?~', '~?erep?~', '{{w_br}}'], ['', '', '<wbr>'], $final);
            $teaser = truncate_comment(preg_replace('#(<br>)+#', ' ', $final), 300, true);
            $teaser_full = strip_tags(preg_replace('/<span class="sjis".+?<\/span>/', '[SJIS]', preg_replace('#(<br>)+#', ' ', $final)), '<s>');
            $cases[] = ['input' => $input, 'admission_input' => $admission_input, 'rolls' => $rolls, 'markup' => $comment, 'filtered' => $filtered,
                'linked' => $linked, 'wrapped' => str_replace('{{w_br}}', '<wbr>', $wrapped), 'final' => $final, 'teaser' => $teaser, 'teaser_full' => $teaser_full];
        }
    }
    echo json_encode($cases, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR);
    exit(0);
}
$profiles = [];
foreach ($hashes as $profile => $hash) {
    $process = proc_open([PHP_BINARY, __FILE__, $root, $argv[2], '--worker', $profile],
        [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes, null, null, ['bypass_shell' => true]);
    if (!is_resource($process)) { throw new RuntimeException('Pure worker did not start.'); }
    fclose($pipes[0]);
    $output = stream_get_contents($pipes[1], 2097153); fclose($pipes[1]);
    $errors = stream_get_contents($pipes[2], 4097); fclose($pipes[2]);
    $status = proc_close($process);
    if ($status !== 0 || strlen($output) > 2097152 || $errors !== '') { throw new RuntimeException('Pure worker failed: ' . $profile); }
    $profiles[$profile] = json_decode($output, true, 64, JSON_THROW_ON_ERROR);
}
$files = ['imgboard.php' => hash('sha256', $app), 'lib/util.php' => hash('sha256', $util),
    'lib/postfilter.php' => hash('sha256', $postfilter)];
foreach ($hashes as $name => $hash) { $files['wordfilters/' . $name . '.php'] = $hash; }
$json = json_encode(['reference' => 'operator-supplied 4chan-old checkout', 'extractor_php' => PHP_VERSION,
    'extractor_pcre' => PCRE_VERSION, 'board' => 'g', 'markup_policy' => ['spoilers' => true, 'code' => true, 'sjis' => true, 'op' => true],
    'files' => $files, 'profiles' => $profiles], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Posting reference differs.'); }
    echo 'Wordfilter posting reference matches ' . array_sum(array_map('count', $profiles)) . " synthetic cases.\n";
} else { file_put_contents($argv[2], $json); }
