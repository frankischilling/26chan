<?php
// Independent source oracle for normalization/static-link lexical boundaries.
// Only the named pure functions are extracted; application/bootstrap/database
// code is never loaded. All inputs below are synthetic.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-source-link-boundary-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$source = file_get_contents($root . '/imgboard.php');
if (hash('sha256', $source) !== 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445') {
    throw new RuntimeException('Audited source link functions differ.');
}
function load_source_function($source, $name) {
    $tokens = token_get_all(preg_replace('/^<\?(?=\s)/', '<?php', $source));
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
            if ($token === '}' && --$depth === 0 && $opened) { eval($body); return; }
        }
    }
    throw new RuntimeException("Missing pure source function: $name");
}
foreach (['normalize_link_cb', 'normalize_links', 'boards_matching_arr',
    'normalize_and_linkify', 'clean_internal_link', 'auto_link_static_cb'] as $name) {
    load_source_function($source, $name);
}
if (!preg_match('/\$valid_boards\s*=\s*"([a-z0-9|]+)";/', $source, $match)) {
    throw new RuntimeException('Static board allowlist is missing.');
}
$valid_boards = $match[1];
define('BOARD_DIR', 'g');
class L { static function d($board) { return '4chan.org'; } }
function decode_source($value) { return html_entity_decode($value, ENT_QUOTES, 'UTF-8'); }
function source_links($linked) {
    preg_match_all('#<a href="([^"]+)"([^>]*)>(.*?)</a>#s', $linked, $matches, PREG_SET_ORDER);
    $links = [];
    foreach ($matches as $match) {
        $links[] = ['href' => preg_replace('#^//(?:boards|www)\.4chan\.org#', '', decode_source($match[1])),
            'label' => decode_source($match[3]), 'new_tab' => strpos($match[2], 'target="_blank"') !== false];
    }
    return $links;
}
function source_case($input) {
    $warnings = [];
    set_error_handler(function ($severity, $message) use (&$warnings) {
        $warnings[] = $message;
        return true;
    });
    $escaped = htmlspecialchars($input, ENT_QUOTES, 'UTF-8');
    $probe = strpos($escaped, '4chan') !== false || strpos($escaped, '4cdn.org') !== false;
    $normalized = $probe ? normalize_links($escaped) : $escaped;
    $linked = normalize_and_linkify($escaped);
    restore_error_handler();
    return ['input' => $input, 'normalized' => decode_source($normalized),
        'links' => source_links($linked), 'warnings' => $warnings];
}
$normalization = [
    'https://boards.4chan.org/g/imgboard.php?res=42',
    'https://boards.4chan.org/g/imgboardXphp?res=42#p43',
    'https://boards.4chan.org/g/xphp?res=42',
    'https://boards.4chan.org/g/xxphp?res=42',
    'https://boards.4chan.org/g/x/php?res=42',
    'https://boards.4chan.org/g/x-php?res=42',
    'https://boards.4chan.org/g/x.php?RES=42#Q43',
    'https://boards.4chan.org/g/x..php?res=42',
    "https://boards.4chan.org/g/x\nphp?res=42",
    "https://boards.4chan.org/g/xéphp?res=42",
    'https://boards.4chan.org/g/thread/0',
    'https://boards.4chan.org/g/res/0',
    'https://boards.4chan.org/g/THREAD/0',
    'https://boards.4chan.org/g/thread/00',
    'https://boards.4chan.org/g/thread/42#p0',
    'https://boards.4chan.org/g/thread/42#q00',
    'https://boards.4chan.org/g/thread/0#p0',
    'https://boards.4chan.org/po/thread/0',
    'https://boards.4chan.org/po/thread/0#p43',
    'https://boards.4chan.org/g/imgboard.php?res=0',
    'https://boards.4chan.org/g/imgboard.php?res=42#0',
    'https://boards.4chan.org/g/imgboard.php?res=0#00',
    'https://boards.4chan.org/g/',
    'https://boards.4chan.org/po/',
    'https://boards.4chan.org/longboard12345/thread/42',
    'https://boards.4chan.org/longboard12345/',
    'https://boards.4chan.org/with_under/thread/42',
    'https://boards.4chan.org/polls/thread/42',
    'https://boards.4chan.org/é/thread/42',
    'https://boards.4chan.org/g/catalog#s=0',
    'https://boards.4chan.org/g/catalog#s=',
    'https://boards.4chan.org/g/CATALOG#S=a+B',
    'https://boards.4chan.org/g/catalogue',
    'https://boards.4chan.org/g/thread/42#p',
    'https://boards.4chan.org/g/thread/42#bad',
    'https://boards.4chan.org/g/thread/42/slug.more',
    'https://boards.4chan.org/g/thread/42/slug/more',
    'https://boards.4chan.org/g/thread/42?query=1',
    'HTTPS://BOARDS.4CHAN.ORG/g/thread/42',
    '4chan HTTPS://BOARDS.4CHAN.ORG/g/thread/42',
    '4chan HTTPS://boards.4CHAN.ORG/g/thread/42',
    'https://www.4chan.org/g/thread/42',
    'https://boards.4chan.org.evil.test/g/thread/42',
    'https://boards.4chan.org:443/g/thread/42',
];
foreach ([" ", "\t", "\n", "\v", "\f", "\r", "\u{0085}", "\u{00a0}", "\u{2003}", "\u{2028}", '.', '!', '?', ',', '/', ':', ';', 'a'] as $boundary) {
    $normalization[] = 'https://boards.4chan.org/g/thread/42' . $boundary . 'tail';
}
// These exercise normalization only. Entity-bearing source server anchors
// have a separate typed-display boundary and are not asserted as raw HTML.
$escaped_normalization = [];
foreach (['<', '>', '&', '"', "'"] as $character) {
    $escaped_normalization[] = 'https://boards.4chan.org/g/x' . $character . 'php?res=42';
    $escaped_normalization[] = 'https://boards.4chan.org/g/thread/42' . $character . 'tail';
}
$static = [
    '>>>/po/1e2', '>>>/po/+1e2', '>>>/po/-1e2', '>>>/po/1e+2', '>>>/po/1e-2',
    '>>>/po/0e0', '>>>/po/00e00', '>>>/po/1e99999999999999999999999999999999',
    '>>>/po/1e', '>>>/po/e2', '>>>/po/1e+', '>>>/po/1e-', '>>>/po/1ee2',
    '>>>/po/1e2e3', '>>>/po/1e2x', '>>>/po/1e2/3', '>>>/po/1e2,3',
    '>>>/po/1E2', '>>>/po/.5', '>>>/po/1.5', '>>>/po/+', '>>>/po/-',
    '>>>/po/++1', '>>>/po/--1', '>>>/po/1e+-2', '>>>/po/0x10',
    '>>>/po/inf', '>>>/po/nan', '>>>/po/0', '>>>/po/00',
    '>>>/po/', '>>>/po/catalog', '>>>/po/a+b/c,d-e',
    '>>>/longboard12345/rules/3', '>>>/longboard12345/catalog',
    '>>>/polls/rules/3', '>>>/polls/catalog', '>>>/unknown/rules',
    '>>>/unknown/', '>>>/with_under/rules', '>>>/G/rules', '>>>//rules',
    '>>>/é/rules', '>>>/../rules', '>>>/g/rules/../../x',
    '>>>/g/rules//elsewhere', '>>>/g/rulesbad/1', '>>>/f/catalog', '>>>/f/rules',
];
$server = [
    'http://4chan.org/', 'https://www.4chan.org/', 'https://www.4chan.org/.',
    'https://www.4chan.org/()', 'https://www.4chan.org//', 'https://www.4chan.org/a.',
    'https://www.4chan.org/a/().,', 'https://i.4cdn.org/g/1.jpg',
    'https://www.4chan.org:443/path', 'https://www.4chan.org.evil.test/path',
    'https://x1.4chan.org/path', 'https://www.4chan.org@evil.test/path',
    'https://user:password@www.4chan.org/path', 'javascript://www.4chan.org/path',
    'https://example.org/4chan',
];
// Execute the post-wrap numeric gate with a recording missing-post stub.
// Database coercion and post existence are deliberately not simulated.
$util = file_get_contents($root . '/lib/util.php');
if (hash('sha256', $util) !== '5307cea255e2d3eb243c7549e1a6521281bf6253c47e010476412b9a4c63a70c') {
    throw new RuntimeException('Audited UTF-8 wrapping source differs.');
}
foreach (['wordwrap2', 'parse_interboard_link'] as $name) { load_source_function($source, $name); }
load_source_function($util, 'utf8_wordwrap');
function other_board_resto($board, $number) {
    $GLOBALS['owned_query_terms'][] = ['board' => $board, 'number' => $number];
    return false;
}
$dynamic_cases = [];
foreach ([0, 25, 26, 27] as $prefix) {
    foreach (['1e2', '1e+2', '1e-2', '42', '+001'] as $term) {
        $input = str_repeat('x', $prefix) . '>>>/po/' . $term;
        $linked = normalize_and_linkify(htmlspecialchars($input, ENT_QUOTES, 'UTF-8'));
        $wrapped = str_replace('{{w_br}}', '<wbr>', wordwrap2($linked, 35, '{{w_br}}'));
        $GLOBALS['owned_query_terms'] = [];
        $rendered = preg_replace_callback('#&gt;&gt;&gt;/([a-z0-9]+)/([a-z0-9+/-]*)#', function ($match) {
            return parse_interboard_link($match[0], $match[1], $match[2]);
        }, $wrapped);
        $dynamic_cases[] = ['input' => $input, 'wrapped' => $wrapped,
            'lookup_terms' => $GLOBALS['owned_query_terms'], 'missing_post_html' => $rendered];
    }
}
$reference = ['reference' => 'operator-supplied 4chan-old checkout',
    'runtime' => 'PHP ' . PHP_VERSION, 'board' => BOARD_DIR,
    'files' => ['imgboard.php' => hash('sha256', $source), 'lib/util.php' => hash('sha256', $util)],
    'scope' => 'Original normalization/static-link functions and post-wrap numeric gate, with recording missing-post stub; database coercion and existence are unqualified.',
    'dynamic_cases' => $dynamic_cases,
    'normalization_cases' => array_map('source_case', $normalization),
    'escaped_normalization_cases' => array_map('source_case', $escaped_normalization),
    'static_cases' => array_map('source_case', $static),
    'server_cases' => array_map('source_case', $server)];
$json = json_encode($reference, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Source link boundary reference differs.'); }
    echo 'Source link boundary reference matches ' . count($normalization) . ' normalization, '
        . count($escaped_normalization) . ' escaped normalization, ' . count($static) . ' static and ' . count($server) . " server-link cases.\n";
} else {
    file_put_contents($argv[2], $json);
}
