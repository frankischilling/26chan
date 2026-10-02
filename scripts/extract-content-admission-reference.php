<?php
// Run only four selected, hash-pinned bodies against synthetic boundary stubs.
// Never load the application, its includes, a database, real sessions or RPC.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-content-admission-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
if (!extension_loaded('intl') || !extension_loaded('mbstring')) {
    throw new RuntimeException('The fixture requires ICU and UTF-8 support.');
}
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$hash = 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207';
$names = ['normalize_ascii', 'strip_zerowidth', 'normalize_text', 'spam_filter_post_content_new'];

if (($argv[3] ?? '') === '--worker') {
    if (!in_array($argv[4] ?? '', ['0', '1'], true)) { throw new RuntimeException('Unknown source constant.'); }
    define('TEST_BOARD', $argv[4] === '1');
    // Fixed sentinels identify branches independently of language configuration.
    define('S_GENERICERROR', 'generic');
    define('S_BANNEDTEXT', 'banned');
    define('S_REJECTTEXT', 'rejected');
    $source = file_get_contents($root . '/lib/postfilter.php');
    if (hash('sha256', $source) !== $hash) { throw new RuntimeException('Audited admission source differs.'); }
    $tokens = token_get_all($source);
    $found = [];
    $selected = '';
    for ($i = 0; $i < count($tokens); ++$i) {
        if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
        $start = $i;
        do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
        if (!is_array($tokens[$i]) || !in_array($tokens[$i][1], $names, true)) { continue; }
        $name = $tokens[$i][1];
        if (isset($found[$name])) { throw new RuntimeException('Repeated audited body.'); }
        $depth = 0; $opened = false; $body = '';
        for ($j = $start; $j < count($tokens); ++$j) {
            $token = $tokens[$j];
            $body .= is_array($token) ? $token[1] : $token;
            if ($token === '{') { ++$depth; $opened = true; }
            if ($token === '}' && --$depth === 0 && $opened) { break; }
        }
        if (!$opened || $depth !== 0 || strlen($body) > 16384) { throw new RuntimeException('Audited body exceeds bounds.'); }
        $selected .= $body . "\n"; $found[$name] = true;
    }
    if (count($found) !== 4 || strlen($selected) > 32768) { throw new RuntimeException('Audited body set is incomplete.'); }

    class OwnedStop extends Exception {
        public function __construct(public string $kind, public ?string $messageText = null) { parent::__construct('Synthetic stop.'); }
    }
    class OwnedCursor { public int $offset = 0; public function __construct(public array $rows) {} }
    class UserPwd {
        public static function getSession() { return $GLOBALS['owned_input']['session'] ? new self() : null; }
        public function isUserKnownOrVerified($minutes) {
            if ($minutes !== 1440) { throw new RuntimeException('Known-user age changed.'); }
            return $GLOBALS['owned_input']['known'];
        }
        public function postCount() { return $GLOBALS['owned_input']['posts']; }
    }
    function mysql_global_call($query, $board) {
        if ($board !== $GLOBALS['owned_input']['board']) { throw new RuntimeException('Source scope changed.'); }
        if (!$GLOBALS['owned_input']['query_available']) { return false; }
        $rows = array_values(array_filter($GLOBALS['owned_input']['rules'],
            fn($rule) => $rule['board'] === '' || $rule['board'] === $board));
        return new OwnedCursor($rows);
    }
    function mysql_fetch_assoc($cursor) { return $cursor->rows[$cursor->offset++] ?? false; }
    function register_postfilter_hit($id) { $GLOBALS['owned_effects']['hits'][] = $id; }
    function log_postfilter_hit($rule, $board, $resto, $name, $sub, $com, $file) {
        $GLOBALS['owned_effects']['logs'][] = ['id' => $rule['id'], 'board' => $board,
            'reply' => $resto !== 0, 'name' => $name, 'subject' => $sub, 'comment' => $com, 'filename' => $file];
    }
    function auto_ban_poster($name, $days, $reject, $private, $public, $automatic = false, $pwd = null, $pass = null) {
        $GLOBALS['owned_effects']['bans'][] = ['days' => $days, 'reject' => $reject, 'automatic' => $automatic,
            'reason' => $public, 'filename_proxy' => $private === 'PHP proxy (via filename check)'];
    }
    function error($message) { throw new OwnedStop('error', $message); }
    function show_post_successful_fake($resto) { throw new OwnedStop('quiet'); }
    eval($selected);

    $base = ['board' => TEST_BOARD ? 'test' : 'demo', 'reply' => false,
        'name' => 'Anonymous', 'subject' => '', 'comment' => 'Paper folding', 'filename' => '',
        'session' => true, 'known' => false, 'posts' => 0, 'query_available' => true, 'rules' => []];
    $rule = ['id' => 1, 'pattern' => 'paper', 'autosage' => false, 'log' => false, 'regex' => false,
        'quiet' => false, 'lenient' => false, 'ops_only' => false, 'min_count' => 1, 'board' => '', 'ban_days' => 0];
    $inputs = [];
    $add = function($label, $changes = [], $rules = null) use (&$inputs, $base) {
        $input = array_replace($base, $changes);
        if ($rules !== null) { $input['rules'] = $rules; }
        $inputs[] = ['label' => $label, 'input' => $input];
    };
    foreach ([false, true] as $reply) {
        foreach ([false, true] as $autosage) {
            foreach ([false, true] as $log) {
                foreach ([false, true] as $quiet) {
                    foreach ([0, 3] as $days) {
                        $add('action precedence', ['reply' => $reply], [array_replace($rule,
                            ['autosage' => $autosage, 'log' => $log, 'quiet' => $quiet, 'ban_days' => $days])]);
                    }
                }
            }
        }
    }
    foreach ([false, true] as $session) {
        foreach ([false, true] as $known) {
            foreach ([0, 10, 11, 100] as $posts) {
                $add('leniency threshold', ['session' => $session, 'known' => $known, 'posts' => $posts],
                    [array_replace($rule, ['lenient' => true])]);
            }
        }
    }
    foreach ([false, true] as $reply) {
        foreach ([false, true] as $opsOnly) {
            $add('OP scope', ['reply' => $reply], [array_replace($rule, ['ops_only' => $opsOnly])]);
        }
    }
    foreach ([-2, 0, 1, 2, 3, 4] as $count) {
        foreach (['paper', 'paper paper', 'paperpaperpaper', 'pāper [spoiler]paper[/spoiler] PAPER'] as $comment) {
            $add('literal count', ['comment' => $comment], [array_replace($rule, ['min_count' => $count])]);
            $add('regexp count', ['comment' => $comment], [array_replace($rule,
                ['regex' => true, 'pattern' => '/paper/i', 'min_count' => $count])]);
        }
    }
    foreach (['phpAA', 'phpé', 'phpAA.jpg', 'PHPAA', 'phpA', "php\nA", 'xphpAA'] as $filename) {
        $add('filename proxy', ['filename' => $filename]);
    }
    foreach (['moot', 'MOOT', 'smooth', 'admin', 'Administrator', 'аdmin', 'mοοt', 'paper', '##', ''] as $subject) {
        $add('fixed subject', ['subject' => $subject]);
        $add('logged fixed subject', ['subject' => $subject], [array_replace($rule, ['log' => true])]);
        $add('autosage before fixed subject', ['subject' => $subject], [array_replace($rule, ['autosage' => true])]);
        $add('source query failure', ['subject' => $subject, 'query_available' => false], [$rule]);
    }
    foreach (['Paper', 'paper', 'Paper Folding', 'Anonymous', 'missing'] as $pattern) {
        foreach (['Paper folding', '[spoiler]Paper[/spoiler] folding', '[SPOILER]Paper[/SPOILER] folding',
                  '[code]Paper[/code] folding', '[sjis]Paper[/sjis] folding', 'Paper...folding',
                  'Paper&gt;folding', 'Paper/!folding', "Paper\tfolding"] as $comment) {
            $add('autosage projection and fallthrough', ['comment' => $comment],
                [array_replace($rule, ['autosage' => true, 'pattern' => $pattern])]);
        }
    }
    foreach ([['name' => 'Pa', 'subject' => 'per', 'comment' => 'fold'],
              ['name' => '', 'subject' => '', 'filename' => 'paper', 'comment' => 'fold'],
              ['name' => 'foil', 'subject' => 'paper', 'comment' => 'fold'],
              ['name' => '', 'subject' => '', 'comment' => 'paperfold']] as $fields) {
        $add('concatenated fields', $fields, [$rule]);
        $add('regexp field separators', $fields, [array_replace($rule, ['regex' => true, 'pattern' => '/paper/i'])]);
    }
    foreach (['/paper/', '/PAPER/i', '/^Anonymous /', '/fold$/', '/paper\s+fold/i',
              '/paper.fold/i', '/paper.fold/is', '/paper(?=fold)/', '/(paper)\1/', '/(?:paper){2}/'] as $pattern) {
        $add('regexp syntax', ['comment' => "paper\nfold paperfold paperpaper"],
            [array_replace($rule, ['regex' => true, 'pattern' => $pattern])]);
    }
    $add('nonoverlapping string count', ['comment' => 'aaaaa'], [array_replace($rule, ['pattern' => 'aa', 'min_count' => 3])]);
    $add('first match order', [], [$rule, array_replace($rule, ['id' => 2, 'log' => true])]);
    $add('first log stops scan', [], [array_replace($rule, ['log' => true]), array_replace($rule, ['id' => 2])]);
    $add('board scope', [], [array_replace($rule, ['board' => 'other'])]);
    $add('board scope', [], [array_replace($rule, ['board' => $base['board']])]);
    $add('empty literal', [], [array_replace($rule, ['pattern' => ''])]);
    $add('no rules');
    if (count($inputs) > 512) { throw new RuntimeException('Admission case count exceeds bounds.'); }
    $cases = [];
    foreach ($inputs as $case) {
        $GLOBALS['owned_input'] = $case['input'];
        $GLOBALS['owned_effects'] = ['hits' => [], 'logs' => [], 'bans' => []];
        $input = $case['input'];
        foreach (['name', 'subject', 'comment', 'filename'] as $field) {
            if (strlen($input[$field]) > 1024) { throw new RuntimeException('Admission input exceeds bounds.'); }
        }
        try {
            $autosage = spam_filter_post_content_new($input['board'], $input['reply'] ? 123 : 0,
                $input['comment'], $input['subject'], $input['name'], $input['filename']);
            $outcome = ['kind' => $autosage ? 'autosage' : 'allow', 'message' => null];
        } catch (OwnedStop $stop) {
            $outcome = ['kind' => $stop->kind, 'message' => $stop->messageText];
        }
        $cases[] = $case + ['outcome' => $outcome, 'effects' => $GLOBALS['owned_effects']];
    }
    $json = json_encode($cases, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR);
    if (strlen($json) > 524288) { throw new RuntimeException('Admission output exceeds bounds.'); }
    echo $json;
    exit(0);
}

$cases = [];
foreach (['0', '1'] as $testBoard) {
    $process = proc_open([PHP_BINARY, __FILE__, $root, $argv[2], '--worker', $testBoard],
        [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes, null, null, ['bypass_shell' => true]);
    if (!is_resource($process)) { throw new RuntimeException('Admission worker failed to start.'); }
    fclose($pipes[0]);
    stream_set_blocking($pipes[1], false); stream_set_blocking($pipes[2], false);
    $output = ''; $errors = ''; $deadline = microtime(true) + 15;
    do {
        $output .= stream_get_contents($pipes[1], 524289 - strlen($output));
        $errors .= stream_get_contents($pipes[2], 4097 - strlen($errors));
        $status = proc_get_status($process);
        if (strlen($output) > 524288 || strlen($errors) > 4096 || microtime(true) > $deadline) {
            proc_terminate($process, 9);
            throw new RuntimeException('Admission worker exceeded bounds.');
        }
        if ($status['running']) { usleep(10000); }
    } while ($status['running']);
    $output .= stream_get_contents($pipes[1], 524289 - strlen($output));
    $errors .= stream_get_contents($pipes[2], 4097 - strlen($errors));
    fclose($pipes[1]); fclose($pipes[2]); proc_close($process);
    if ($status['exitcode'] !== 0 || strlen($output) > 524288 || $errors !== '') {
        throw new RuntimeException('Synthetic admission worker failed.');
    }
    foreach (json_decode($output, true, 64, JSON_THROW_ON_ERROR) as $case) {
        $cases[] = ['test_board' => $testBoard === '1'] + $case;
    }
}
$json = json_encode(['reference' => 'operator-supplied 4chan-old checkout', 'functions' => $names,
    'files' => ['lib/postfilter.php' => $hash], 'extractor_php' => PHP_VERSION,
    'extractor_pcre' => PCRE_VERSION, 'extractor_icu' => INTL_ICU_VERSION,
    'boundary_stubs' => ['database rows', 'known-session decision', 'hit/log/ban capture', 'terminal error/fake-success'],
    'cases' => $cases], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (strlen($json) > 1048576) { throw new RuntimeException('Admission fixture exceeds bounds.'); }
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Content admission reference differs.'); }
} elseif (file_put_contents($argv[2], $json) !== strlen($json)) {
    throw new RuntimeException('Admission fixture could not be saved.');
}
echo 'Content admission source cases: ' . count($cases) . "\n";
