<?php
// Run only selected pure formatting functions on synthetic input. The source
// application, configuration, database, and request handlers are never loaded.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-format-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$source = file_get_contents($root . '/imgboard.php');
$util = file_get_contents($root . '/lib/util.php');
function load_pure_function($source, $name) {
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
            if ($token === '{') { ++$depth; $opened = true; }
            if ($token === '}' && --$depth === 0 && $opened) { eval($body); return; }
        }
    }
    throw new RuntimeException("Missing pure reference function: $name");
}
foreach (['normalize_link_cb', 'normalize_links', 'boards_matching_arr', 'normalize_and_linkify',
    'clean_internal_link', 'auto_link_static_cb', 'wordwrap2', 'truncate_comment'] as $function) {
    load_pure_function($source, $function);
}
load_pure_function($util, 'utf8_wordwrap');
if (!preg_match('/\$valid_boards\s*=\s*"([a-z0-9|]+)";/', $source, $match)) {
    throw new RuntimeException('Static board allowlist is missing.');
}
$valid_boards = $match[1];
define('BOARD_DIR', 'g');
define('SJIS_TAGS', true);
class L { static function d($board) { return '4chan.org'; } }
mb_internal_encoding('UTF-8');
$inputs = [
    'https://boards.4chan.org/g/thread/42',
    'http://boards.4channel.org/G/res/42/a-slug#p43',
    'https://boards.4chan.org/po/thread/42#q43',
    'https://boards.4chan.org/g/imgboard.php?res=42#43',
    'https://boards.4chan.org/g/thread/42#p',
    'https://boards.4chan.org/g/catalog#s=a+b',
    'https://boards.4chan.org/g/catalog#s=ABC',
    'https://boards.4chan.org/g/',
    'https://boards.4chan.org/g/thread/42!!!',
    'https://boards.4chan.org/g/thread/42/slug!tail',
    'HTTPS://BOARDS.4CHAN.ORG/g/thread/42',
    '4chan HTTPS://boards.4CHAN.ORG/g/thread/42',
    '4chan HTTPS://BOARDS.4CHAN.ORG/g/thread/42',
    'https://boards.4chan.org/g/thread/42#bad',
    'https://boards.4chan.org/g/thread/42/no.valid',
    'https://boards.4chan.org/g/thread/42/slug/more',
    'https://boards.4chan.org/g/thread/42?query=1',
    'https://www.4chan.org/faq https://example.org/elsewhere',
    'https://i.4cdn.org/g/1234567.jpg',
    'http://4chan.org http://www.4channel.org/rules?x=1&y=2.',
    'https://www.4chan.org/faq/(test).,',
    'https://www.4chan.org/derefer?url=https://example.org/a',
    'https://www.4chan.org/derefer https://i.4cdn.org/g/1.jpg',
    'https://example.org/ https://EXAMPLE.org/Path?x=1&y=2',
    'https://user:password@www.4chan.org/faq',
    'https://www.4chan.org.evil.test/path',
    '>>>/po/ >>>/g/catalog >>>/g/a+b/c,d-e',
    '>>>/unknown/catalog >>>/j/ >>>/test/ >>>/asp/',
    '>>>/g/rules >>>/g/rules/3 >>>/unknown/rules/4',
    '>>>/f/catalog >>>/g/42 >>>/g/+42 >>>/g/-42 >>>/g/123abc',
    '>>>/g/rulesbad >>>/g/CATALOG >>>/G/catalog',
    str_repeat('界', 70) . ' ' . str_repeat('x', 70),
    '>>>/g/' . str_repeat('x', 70),
    'https://www.4chan.org/' . str_repeat('x', 70),
    '<script>alert("owned")</script> https://www.4chan.org/faq',
];
$cases = [];
foreach ($inputs as $input) {
    $warnings = [];
    set_error_handler(function ($severity, $message) use (&$warnings) { $warnings[] = $message; return true; }, E_WARNING);
    $escaped = htmlspecialchars($input, ENT_QUOTES, 'UTF-8');
    $probe = strpos($escaped, '4chan') !== false || strpos($escaped, '4cdn.org') !== false;
    $normalized = $probe ? normalize_links($escaped) : $escaped;
    $linked = normalize_and_linkify($escaped);
    preg_match_all('#<a href="([^"]+)"([^>]*)>(.*?)</a>#s', $linked, $matches, PREG_SET_ORDER);
    $links = [];
    foreach ($matches as $match) {
        $href = html_entity_decode($match[1], ENT_QUOTES, 'UTF-8');
        $href = preg_replace('#^//(?:boards|www)\.4chan\.org#', '', $href);
        $links[] = ['href' => $href,
            'label' => html_entity_decode($match[3], ENT_QUOTES, 'UTF-8'),
            'new_tab' => strpos($match[2], 'target="_blank"') !== false];
    }
    $cases[] = ['input' => $input,
        'normalized' => html_entity_decode($normalized, ENT_QUOTES, 'UTF-8'), 'links' => $links, 'warnings' => $warnings];
    restore_error_handler();
}
$teasers = [];
foreach ([
    ["[spoiler]hidden[/spoiler]\n\n> green  <script>&'\"", '<s>hidden</s><br><br><span class="quote">&gt; green  &lt;script&gt;&amp;&#039;&quot;</span>'],
    ["[sjis]a\nb[/sjis] tail", '<span class="sjis">a<br>b</span> tail'],
    ['[sjis]a[b]b[/b]c[/sjis] tail', '<span class="sjis">a<span class="mu-s">b</span>c</span> tail'],
    ['[b]short[/b]', '<span class="mu-s">short</span>'],
    [str_repeat('x', 300), str_repeat('x', 300)], [str_repeat('x', 301), str_repeat('x', 301)],
    [str_repeat('x', 298) . '&tail', str_repeat('x', 298) . '&amp;tail'],
    ['[spoiler]' . str_repeat('界', 301) . '[/spoiler]', '<s>' . str_repeat('界', 301) . '</s>'],
    [str_repeat('[b]a[/b]', 12), str_repeat('<span class="mu-s">a</span>', 12)],
    ['[sjis]' . str_repeat('界', 400) . '[/sjis]', '<span class="sjis">' . str_repeat('界', 400) . '</span>'],
    ['>>>/g/catalog', normalize_and_linkify('&gt;&gt;&gt;/g/catalog')],
    ['https://www.4chan.org/faq', normalize_and_linkify('https://www.4chan.org/faq')],
    ['>>42 >>>/po/43', '&gt;&gt;42 &gt;&gt;&gt;/po/43'],
] as [$input, $comment]) {
    $comment = str_replace('{{w_br}}', '<wbr>', wordwrap2($comment, 35, '{{w_br}}'));
    foreach ([false, true] as $text_only) {
        foreach ([false, true] as $truncate) {
            $converted = preg_replace('#(<br>)+#', $text_only ? "\n" : ' ', $comment);
            $teaser = $truncate ? truncate_comment($converted, 300, true)
                : strip_tags(preg_replace('/<span class="sjis".+?<\/span>/', '[SJIS]', $converted), '<s>');
            $teasers[] = ['input' => $input, 'format' => 125, 'comment' => $comment, 'text_only' => $text_only, 'truncate' => $truncate, 'teaser' => $teaser];
        }
    }
}
$reference = ['reference' => 'operator-supplied 4chan-old checkout', 'runtime' => 'PHP ' . PHP_VERSION . ', UTF-8 mbstring',
    'board' => BOARD_DIR, 'sjis' => SJIS_TAGS, 'files' => ['imgboard.php' => hash('sha256', $source),
        'catalog.php' => hash_file('sha256', $root . '/catalog.php'), 'lib/util.php' => hash('sha256', $util)],
    'link_cases' => $cases, 'teaser_cases' => $teasers];
$json = json_encode($reference, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Formatting reference differs.'); }
    echo 'Formatting reference matches ' . count($cases) . ' link and ' . count($teasers) . " teaser cases.\n";
} else {
    file_put_contents($argv[2], $json);
}
