<?php
// Only audited, hash-pinned pure wordfilter files run on bounded synthetic data.
// Never load imgboard.php, postfilter.php, configuration, RPC or a database.
if ($argc < 3) { fwrite(STDERR, "Usage: php scripts/extract-wordfilter-reference.php SOURCE OUTPUT [--check]\n"); exit(2); }
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$hashes = [
    'asp' => '90999f8aebb36bfb516ce3da3d41c7fd1bfff142273ec90f5bf10af46505c5f0',
    'ck' => '18f751bc00a43986ce8b73b32c885240b600959abdd993ad2f9ac53fcf29d952',
    'global' => 'f5ff2ac132b86fa4889e4312f3c2162ab121854cffeb33505f22bfbd2c65b439',
    'int' => '18f751bc00a43986ce8b73b32c885240b600959abdd993ad2f9ac53fcf29d952',
    'test' => '6eebf9ea022c07ae445097fee1be1951fc723dbf03f51ac62df790eee48eb6d5',
    'v' => '5801c47c10383f99eb5826b8625c3d4cc3d808ec4ced74a6d2c3ae3f1eae13d6',
    'vg' => 'f5ff2ac132b86fa4889e4312f3c2162ab121854cffeb33505f22bfbd2c65b439',
    'vp' => 'f5ff2ac132b86fa4889e4312f3c2162ab121854cffeb33505f22bfbd2c65b439',
];
if (($argv[3] ?? '') === '--worker') {
    $profile = $argv[4] ?? '';
    if (!isset($hashes[$profile])) { throw new RuntimeException('Unknown pure filter.'); }
    $source = file_get_contents($root . '/wordfilters/' . $profile . '.php');
    if (hash('sha256', $source) !== $hashes[$profile]) { throw new RuntimeException('Audited pure filter differs.'); }
    if ($profile === 'test') {
        // Replace only the two random draws with fixed choices. Every pure
        // transformation branch stays unchanged, including the special t rule.
        foreach ([1, 2] as $number) {
            $draw = '$roll' . $number . ' = mt_rand(0, 5);';
            if (substr_count($source, $draw) !== 1) { throw new RuntimeException('Random draw changed.'); }
            $source = str_replace($draw, '$roll' . $number . ' = $GLOBALS["owned_filter_rolls"][' . ($number - 1) . '];', $source);
        }
        $GLOBALS['owned_filter_rolls'] = [0, 0];
    }
    eval(substr($source, strlen('<?php')));
    $inputs = [
        'smh SMH Smh sMh tbh TBH Tbh sjw SJWS',
        'fam FAM Fam fAm fams FAMS FAMs Fams fAms',
        'family _fam fam_ 1fam fam1 (fam) /fam/ caféfam famé',
        'CUCK cuck Cuck CUCKCUCK',
        'soy Soy SOY sOy soybeans SoyBeans SOYBEANS',
        'sooy soooy ' . 's' . str_repeat('o', 34) . 'ybeans ' . 's' . str_repeat('o', 35) . 'ybeans',
        'soyz soyab soyabc soyabcd soyabcde',
        'soyuz SOYUZ Soyuz soyim SOYIM soylent SOYLENT soylentil',
        's0y sοy sоy sօy sჿy sΟy sОy sՕy sυy',
        'soу soУ soΥ soυ soуbeans soΥbeans',
        'ſoy xsoy soy1 soy_ _soy 1soy',
        'ésoy soyé 日本soy soy日本 soyéab',
        "soy\u{0301} \u{0301}soy soy\u{203F} \u{203F}soy soy² ²soy soy\u{200C}",
        'sᲿy sSoyoY ſoy ſOOY SοY SΟY',
        '&gt;soy<br><s>smh CUCK tbh</s> &amp;fam',
        '<pre class="prettyprint">soy fam CUCK</pre>',
        '[code]soy fam CUCK[/code] https://example.invalid/fam?soy=CUCK',
        'finna Finna Finna FINNA finnas finnaFINNA',
        'finna! Finna! Finna next Finna  next',
        'pcfat pcuck pccuck valvedrone PCFAT Pcfat Pcuck PCuck Pccuck Valvedrone',
        'sonypony Sonypony sonyponies Sonyponies sonybrony Sonybrony sonybronies Sonybronies',
        'sonydrone Sonycuck nintendrone Nintencuck nintoddler Nintendotoddler',
        'nintenyearold Nintendroid nintenshit nintendr0ne Nintendr0ne xpcfatx',
        '', 'ordinary unchanged', "fam\nsoy\tsmh",
    ];
    $cases = [];
    foreach ($inputs as $input) {
        foreach (['com', 'sub', 'name'] as $type) {
            $cases[] = ['input' => $input, 'field' => $type, 'output' => word_filter($input, $type)];
        }
    }
    if ($profile === 'v') {
        foreach ((new ReflectionFunction('word_filter_consoles'))->getStaticVariables()['from'] as $input) {
            $cases[] = ['input' => $input, 'field' => 'com', 'output' => word_filter($input, 'com')];
            $wrapped = 'prefix' . $input . 'suffix';
            $cases[] = ['input' => $wrapped, 'field' => 'com', 'output' => word_filter($wrapped, 'com')];
        }
    }
    $leet = [];
    if ($profile === 'test') {
        foreach (['aeiosTt gt lt tt Tt <pre class="prettyprint">Text</pre>', 'At tt gT lT gt lt T t test', '&amp; &lt; &gt; [code] [/code]'] as $input) {
            for ($first = 0; $first < 6; ++$first) {
                for ($second = 0; $second < 6; ++$second) {
                    $GLOBALS['owned_filter_rolls'] = [$first, $second];
                    $leet[] = ['input' => $input, 'rolls' => [$first, $second], 'output' => april_leet_filter($input)];
                }
            }
        }
    }
    echo json_encode(['cases' => $cases, 'leet' => $leet], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR);
    exit(0);
}
$profiles = []; $files = [];
foreach ($hashes as $profile => $hash) {
    $process = proc_open([PHP_BINARY, __FILE__, $root, $argv[2], '--worker', $profile],
        [0 => ['pipe', 'r'], 1 => ['pipe', 'w'], 2 => ['pipe', 'w']], $pipes, null, null, ['bypass_shell' => true]);
    if (!is_resource($process)) { throw new RuntimeException('Pure filter worker failed to start.'); }
    fclose($pipes[0]);
    $output = stream_get_contents($pipes[1], 65537); fclose($pipes[1]);
    $errors = stream_get_contents($pipes[2], 4097); fclose($pipes[2]);
    $status = proc_close($process);
    if ($status !== 0 || strlen($output) > 65536 || $errors !== '') { throw new RuntimeException('Pure filter worker failed: ' . $profile); }
    $profiles[$profile] = json_decode($output, true, 64, JSON_THROW_ON_ERROR);
    $files['wordfilters/' . $profile . '.php'] = $hash;
}
$files['imgboard.php'] = hash_file('sha256', $root . '/imgboard.php');
$json = json_encode(['reference' => 'operator-supplied 4chan-old checkout', 'extractor_php' => PHP_VERSION,
    'extractor_pcre' => PCRE_VERSION, 'files' => $files, 'profiles' => $profiles],
    JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Wordfilter reference differs.'); }
    echo "Wordfilter reference matches eight pure profiles and all 36 random-choice pairs.\n";
} else { file_put_contents($argv[2], $json); }
