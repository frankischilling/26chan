<?php
// Regenerate inert reference HTML from the supplied, hash-pinned PHP templates.
// This script does not load the absent controller or the absolute footer include.
declare(strict_types=1);

define('IN_APP', true);
date_default_timezone_set('UTC');
$_SERVER['REQUEST_TIME'] = (new DateTimeImmutable('2026-01-01T00:00:00Z'))->getTimestamp();
$root = dirname(__DIR__) . '/tests/fixtures/polls';
foreach ([
    'source-polls.css' => '6baf99fbbb66468b8e10f1c9b333c942cd49b6d22570c6edbf6f988b4da275fb',
    'source-polls.js' => '4b480887bc189760a5dbb38bbe8422e6964c69470b5791a2bf0eff1155cb33d6',
] as $file => $digest) {
    if (hash_file('sha256', $root . '/' . $file) !== $digest) {
        throw new RuntimeException('Referenced asset digest mismatch: ' . $file);
    }
}
if (hash_file('sha256', dirname(__DIR__) . '/apps/public/static/themes/fade.png') !==
    '5f7a2be79027d3a5c7207de3e7efe510bcc4a66f105e174d1000cbffd6e4a274') {
    throw new RuntimeException('Referenced background digest mismatch.');
}
$data = json_decode(file_get_contents($root . '/data.json'), true, 512, JSON_THROW_ON_ERROR);
if ($data['copyright_year'] !== 2026) {
    throw new RuntimeException('The fixture timestamp and copyright year must agree.');
}

final class PollReference {
    public array $items;
    public array $poll;
    public array $options;
    public ?array $scores = null;
    public string $_tkn;
    public int $poll_id;

    public function __construct(array $data) {
        $this->items = $data['catalogue'];
        $this->poll = $data['poll'];
        $this->options = $this->poll['options'];
        $this->_tkn = $data['token'];
        $this->poll_id = $this->poll['id'];
    }

    public function render(string $root, string $template): string {
        $hashes = [
            'source-polls.tpl.php' => 'e75ae9df16eee5e471a1000155ffa14bf3170aaae084c8f76fecf241ff49037d',
            'source-polls-view.tpl.php' => '4281918d8734fdfaf5758f8963b0453ff857a2ed6d0cc171c1741c8704c2fd14',
        ];
        $source = file_get_contents($root . '/' . $template);
        if (!isset($hashes[$template]) || hash('sha256', $source) !== $hashes[$template]) {
            throw new RuntimeException('Source template digest mismatch.');
        }
        $include = "include '/www/4chan.org/web/www/frontpage_footer.php';";
        if (substr_count($source, $include) !== 1) {
            throw new RuntimeException('Expected exactly one external footer include.');
        }
        $source = str_replace($include, '', $source);
        ob_start();
        try {
            eval('?>' . $source);
            $html = ob_get_contents();
        } finally {
            ob_end_clean();
        }
        // These scripts do not render the page. Never execute analytics or write
        // a source-domain cookie while inspecting the synthetic reference.
        $html = preg_replace('~\s*<script\b[^>]*>.*?</script>~s', '', $html, -1, $count);
        if ($count !== ($template === 'source-polls.tpl.php' ? 1 : 2)) {
            throw new RuntimeException('Unexpected source script count.');
        }
        $html = str_replace(
            ['//s.4cdn.org/css/polls.css?15', '//s.4cdn.org/image/favicon.ico'],
            ['/poll-visual/reference.css', '/static/notifications/favicon.ico'],
            $html,
            $count
        );
        if ($count !== 2 || str_contains($html, '<script') || str_contains($html, '//s.4cdn.org')) {
            throw new RuntimeException('Unexpected external reference.');
        }
        return str_replace("\r\n", "\n", $html);
    }
}

foreach (['catalogue', 'empty-catalogue', 'options', 'options-no-description', 'results', 'empty-results'] as $name) {
    $reference = new PollReference($data);
    $catalogue = str_contains($name, 'catalogue');
    if ($name === 'empty-catalogue') $reference->items = [];
    if ($name === 'options-no-description') $reference->poll['description'] = '';
    if (str_contains($name, 'results')) {
        $reference->scores = [];
        foreach ($reference->options as $option) {
            if ($option['score'] !== null) $reference->scores[$option['id']] = $option['score'];
        }
    }
    if ($name === 'empty-results') {
        $reference->options = [];
        $reference->poll['description'] = '';
        $reference->poll['vote_count'] = 0;
    }
    $html = $reference->render($root, $catalogue ? 'source-polls.tpl.php' : 'source-polls-view.tpl.php');
    $path = $root . '/' . $name . '.html';
    if (($argv[1] ?? '') === '--check') {
        if (!is_file($path) || file_get_contents($path) !== $html) {
            throw new RuntimeException('Reference HTML differs: ' . $name);
        }
    } else {
        file_put_contents($path, $html);
    }
    echo $name . ": " . hash('sha256', $html) . PHP_EOL;
}
