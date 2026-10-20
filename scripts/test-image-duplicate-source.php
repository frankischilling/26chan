<?php
// Execute the unchanged source gate in separate constant profiles. All rows,
// identifiers, digests and URLs below belong to this synthetic fixture.
$profile = $argv[1] ?? '';
if (!in_array($profile, ['ordinary', 'repost', 'history'], true)) {
    $all = [];
    foreach (['ordinary', 'repost', 'history'] as $mode) {
        $command = escapeshellarg(PHP_BINARY) . ' -n ' . escapeshellarg(__FILE__) . ' ' . escapeshellarg($mode);
        $lines = []; $status = 0;
        exec($command, $lines, $status);
        if ($status !== 0) throw new Exception('Source child failed');
        $all = array_merge($all, json_decode(implode("\n", $lines), true, 512, JSON_THROW_ON_ERROR));
    }
    $encoded = json_encode($all, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR) . "\n";
    $path = __DIR__ . '/../crates/domain/tests/fixtures/image-duplicates.json';
    if ($profile === '--write') file_put_contents($path, $encoded);
    elseif (file_get_contents($path) !== $encoded) throw new Exception('Source image duplicate fixture differs');
    echo 'Source image duplicate cases: ' . count($all) . " passed\n";
    exit;
}
define('MAX_IMG_REPOST_COUNT', $profile === 'repost' ? 3 : 0);
if ($profile === 'history') define('SQLLOGMD5', 'owned_md5');
define('SQLLOG', 'owned_posts'); define('BOARD_DIR', 'owned'); define('PHP_EXT2', '');
define('S_DUPE', 'Error: Duplicate file exists.');
class L { static function d($board) { return 'example.invalid'; } }
class DuplicateRefusal extends Exception {}
function error($message, $dest = null) { throw new DuplicateRefusal($message); }
function mysql_num_rows($rows) { return count($rows); }
function mysql_fetch_row($rows) { return $rows[0]; }
$rows = []; $history_age = null; $queries = [];
function mysql_board_call($query, ...$args) {
    global $rows, $history_age, $queries;
    $queries[] = $query;
    if (str_contains($query, 'SELECT sql_no_cache `no`,`resto`')) {
        if (!str_contains($query, 'archived = 0') || !str_contains($query, 'filedeleted=0 limit 1')) throw new Exception('Scope changed');
        $reply = count($args) === 3;
        $target = $reply ? $args[0] : 0;
        $digest = $args[$reply ? 2 : 0];
        if ($reply && $args[1] !== $target) throw new Exception('Thread scope differs');
        foreach ($rows as $row) {
            if (!$row['archived'] && !$row['deleted'] && $row['md5'] === $digest
                && ($reply ? ($row['resto'] === $target || $row['id'] === $target) : $row['resto'] === 0)) return [[$row['id'], $row['resto']]];
        }
        return [];
    }
    if (str_starts_with($query, 'SELECT COUNT(*)')) {
        if (!str_contains($query, 'archived = 0 AND resto != 0') || !str_contains($query, 'filedeleted = 0')) throw new Exception('Repost scope changed');
        return [[count(array_filter($rows, fn($row) => !$row['archived'] && !$row['deleted'] && $row['resto'] !== 0 && $row['md5'] === $args[0]))]];
    }
    if (str_contains($query, 'now > DATE_SUB(NOW(), INTERVAL 1 DAY) limit 1')) {
        return $history_age !== null && $history_age < 86400 ? [[1700000000 - $history_age, 'owned.png', $args[1]]] : [];
    }
    throw new Exception('Unexpected source query');
}
$source = json_decode(file_get_contents(__DIR__ . '/../fixtures/image-duplicate-source.json'), true, 512, JSON_THROW_ON_ERROR);
eval($source['gate_php']);
$cases = [];
foreach ([false, true] as $image) foreach (['none', 'verified'] as $badge) foreach ([0, 10] as $target) {
    foreach (['empty', 'same_active', 'same_archived', 'same_deleted', 'other_thread', 'op', 'count_below', 'count_exact', 'count_above', 'recent_history', 'history_boundary', 'history_old'] as $scenario) {
        $rows = []; $history_age = null; $queries = [];
        $row = fn($id, $resto, $archived = false, $deleted = false) => ['id'=>$id,'resto'=>$resto,'archived'=>$archived,'deleted'=>$deleted,'md5'=>'owned-digest'];
        if (str_starts_with($scenario, 'same_')) $rows[] = $row(11, 10, $scenario==='same_archived', $scenario==='same_deleted');
        if ($scenario==='other_thread') $rows[]=$row(21,20);
        if ($scenario==='op') $rows[]=$row(10,0);
        if (str_starts_with($scenario,'count_')) for ($n=0;$n<['count_below'=>2,'count_exact'=>3,'count_above'=>4][$scenario];$n++) $rows[]=$row(21+$n,20);
        if ($scenario==='recent_history') $history_age=86399;
        if ($scenario==='history_boundary') $history_age=86400;
        if ($scenario==='history_old') $history_age=86401;
        $result='allow';
        try { source_image_duplicate($image,$badge,$target,'owned-digest','owned-file'); }
        catch (DuplicateRefusal $error) {
            $result=str_contains($error->getMessage(), '<a href=') ? 'duplicate' : ($error->getMessage()===S_DUPE ? 'repost' : 'recent');
        }
        $cases[]=['profile'=>$profile,'image'=>$image,'badge'=>$badge,'target'=>$target,'scenario'=>$scenario,'result'=>$result,'queries'=>count($queries)];
    }
}
echo json_encode($cases, JSON_THROW_ON_ERROR);
