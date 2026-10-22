<?php
// Only the pinned flood branch and duration formatter execute. SQL is modeled
// with owned synthetic rows; deployed database collation remains unverified.
error_reporting(E_ALL & ~E_DEPRECATED);
$mode = $argv[1] ?? '';
if (!in_array($mode, ['ordinary', 'meta'], true)) {
    $all = [];
    foreach (['ordinary', 'meta'] as $profile) {
        $output = []; $status = 0;
        exec(escapeshellarg(PHP_BINARY) . ' -n ' . escapeshellarg(__FILE__) . ' ' . $profile, $output, $status);
        if ($status !== 0) throw new Exception('Source child failed');
        $all = array_merge($all, json_decode(implode("\n", $output), true, 512, JSON_THROW_ON_ERROR));
    }
    $encoded = json_encode($all, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
    $path = __DIR__ . '/../crates/domain/tests/fixtures/comment-duplicates.json';
    if ($mode === '--write') file_put_contents($path, $encoded);
    elseif (file_get_contents($path) !== $encoded) throw new Exception('Comment fixture differs');
    echo 'Source duplicate comment cases: ' . count($all) . " passed\n";
    exit;
}
define('META_BOARD', $mode === 'meta');
define('RENZOKU_DUPE', 300);
define('SQLLOG', 'owned_board');
define('S_RENZOKU_DUP', 'Error: You must wait %s before posting a duplicate reply.');
class Refusal extends Exception {}
function error($message, $dest) { throw new Refusal($message); }
function has_level($level = 'mod') {
    global $role;
    return $role === 'moderator' || ($level === 'janitor' && $role === 'janitor');
}
function mysql_board_call($query, $board, $comment, $host, $after) {
    global $times;
    if ($board !== 'owned_board' || $host !== 'owned_peer'
        || !str_contains($query, "where com='%s'") || !str_contains($query, "and host='%s'")
        || !str_contains($query, 'and time>%d')) throw new Exception('Unexpected SQL scope');
    $matching = array_filter($times, fn($time) => $time > $after);
    return $matching ? max($matching) : null;
}
function mysql_result($result, $row, $column) { return $result; }
function mysql_free_result($result) {}
$source = json_decode(file_get_contents(__DIR__ . '/../fixtures/comment-duplicate-source.json'), true, 512, JSON_THROW_ON_ERROR);
eval($source['gate_php']);
eval($source['duration_php']);
$all = [];
foreach (['ordinary', 'vip', 'pass', 'janitor', 'moderator'] as $role)
foreach ([false, true] as $quiet)
foreach (['', '0', 'body'] as $comment)
foreach (['', '0', 'name'] as $name)
foreach ([[], [700], [701], [999], [1000], [1040], [999, 701]] as $times) {
    $_POST['name'] = $name;
    $result = null;
    try { source_duplicate($quiet, $comment, 'owned_peer', 1000, null); }
    catch (Refusal $e) { $result = $e->getMessage(); }
    $all[] = ['meta' => META_BOARD, 'role' => $role, 'quiet' => $quiet, 'comment' => $comment,
        'name' => $name, 'times' => $times, 'result' => $result];
}
echo json_encode($all, JSON_THROW_ON_ERROR);
