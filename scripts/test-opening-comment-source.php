<?php
// Execute only the hash-pinned COM_REGEX branch, with synthetic final comments.
error_reporting(E_ALL);
$source = json_decode(file_get_contents(__DIR__ . '/../fixtures/opening-comment-source.json'), true, 512, JSON_THROW_ON_ERROR);
$mode = $argv[1] ?? '';
if (!isset($source['boards'][$mode])) {
    $all = [];
    foreach (array_keys($source['boards']) as $board) {
        $output = []; $status = 0;
        exec(escapeshellarg(PHP_BINARY) . ' -n ' . escapeshellarg(__FILE__) . ' ' . escapeshellarg($board), $output, $status);
        if ($status !== 0) throw new Exception('Source child failed');
        $all = array_merge($all, json_decode(implode("\n", $output), true, 512, JSON_THROW_ON_ERROR));
    }
    $encoded = json_encode($all, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
    $path = __DIR__ . '/../crates/domain/tests/fixtures/opening-comment-rules.json';
    if ($mode === '--write') file_put_contents($path, $encoded);
    elseif (file_get_contents($path) !== $encoded) throw new Exception('Opening comment fixture differs');
    echo 'Source opening comment cases: ' . count($all) . " passed\n";
    exit;
}
define('COM_REGEX', $source['boards'][$mode]['pattern']);
define('S_INVALID_COM', $source['boards'][$mode]['refusal']);
class Refusal extends Exception {}
function error($message) { throw new Refusal($message); }
eval('function source_gate($resto, $com) {' . $source['gate_php'] . '}');
$all = [];
foreach ([0, 42] as $parent)
foreach (['', '0', 'body', 'http://', 'https://example.test', 'xhttps://x', 'HTTP://x', 'Https://x',
    'ftp://x', 'https:/ /x', 'https&#58;//x', 'http<wbr>://x',
    '<a href="http://x">link</a>', '[code]http://x[/code]', "prefix\nhttps://x", '<span>https://</span>'] as $comment) {
    $result = null;
    try { source_gate($parent, $comment); }
    catch (Refusal $error) { $result = $error->getMessage(); }
    $all[] = ['board' => $mode, 'parent' => $parent, 'pattern' => COM_REGEX, 'comment' => $comment, 'result' => $result];
}
echo json_encode($all, JSON_THROW_ON_ERROR);
