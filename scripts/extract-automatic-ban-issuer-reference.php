<?php
namespace AutomaticBanFixture;
// Execute one pinned function with synthetic SQL, clock, DNS and identity data.
// No application bootstrap, deployment settings, credentials or network calls.
if ($argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
    fwrite(STDERR, "Usage: php extract-automatic-ban-issuer-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
$pin = '571d154aa43cd51ee17bb4dfec5502d6dd63c265f4a2526820c011c934c7d185';
$source = file_get_contents($argv[1] . '/lib/admin.php');
if (hash('sha256', $source) !== $pin) throw new \RuntimeException('Audited source differs.');
if (preg_match_all('/^function auto_ban_poster\(.*?^}/ms', $source, $matches) !== 1
    || strlen($matches[0][0]) > 8192) throw new \RuntimeException('Selected function differs.');
eval('namespace AutomaticBanFixture;' . $matches[0][0]);
unset($source, $matches);
define('S_ANONAME', 'Anonymous');
define('BOARD_DIR', 'test');
define('SQLLOGBAN', 'synthetic_bans');
define('S_SQLFAIL', 'Synthetic SQL failure');
$_SERVER = ['REMOTE_ADDR' => '192.0.2.17'];
function time() { return $GLOBALS['clock']; }
function date($format, $timestamp) {
    return (new \DateTimeImmutable('@' . $timestamp))->setTimezone(new \DateTimeZone($GLOBALS['zone']))->format($format);
}
function gethostbyaddr($host) {
    if ($host !== '192.0.2.17') throw new \RuntimeException('Unexpected synthetic address.');
    return 'synthetic.invalid';
}
function mysql_real_escape_string($value) {
    if (strpbrk($value, "'\\\r\n") !== false) throw new \RuntimeException('Unexpected synthetic SQL value.');
    return $value;
}
function whitelisted_ip() { return $GLOBALS['exempt']; }
function mysql_global_call($sql) {
    $GLOBALS['queries'][] = $sql;
    return true;
}
set_error_handler(function() { throw new \RuntimeException('Unexpected selected-source warning.'); });
$cases = [];
// Midnight, month/year rollover, leap day and both US DST transitions.
foreach (['2024-01-01T00:00:00Z', '2024-02-28T23:59:59Z', '2024-03-10T06:59:59Z',
    '2024-03-10T07:00:00Z', '2024-11-03T05:59:59Z', '2024-11-03T06:00:00Z',
    '2024-12-31T23:59:59Z'] as $instant) {
    $clock = (new \DateTimeImmutable($instant))->getTimestamp();
    foreach (['UTC', 'America/New_York', 'Asia/Tokyo'] as $zone)
    foreach ([-2, -1, 0, 1, 3] as $days)
    foreach ([false, true] as $exempt)
    foreach ([false, true] as $identity) {
        $queries = [];
        auto_ban_poster('Synthetic', $days, true, 'Synthetic reason', 'Public reason', true,
            $identity ? 'synthetic-password' : null, $identity ? 'synthetic-pass' : null);
        if (count($queries) !== ($exempt ? 0 : 1)) throw new \RuntimeException('Unexpected insert count.');
        $row = null;
        if (!$exempt) {
            if (preg_match("/^INSERT INTO synthetic_bans \\(([^)]+)\\) VALUES\\((.*)\\)$/s", $queries[0], $parts) !== 1)
                throw new \RuntimeException('Unexpected insert shape.');
            $columns = explode(',', $parts[1]);
            // All synthetic strings are comma/quote-free. NOW() is retained as a DB clock expression.
            $values = array_map(fn($value) => trim($value, "'"), explode(',', $parts[2]));
            if (count($columns) !== count($values)) throw new \RuntimeException('Unexpected synthetic values.');
            $row = array_combine($columns, $values);
            if (isset($row['active']) || isset($row['now'])) throw new \RuntimeException('Source changed default-owned fields.');
        }
        $cases[] = ['clock' => $clock, 'timezone' => $zone, 'days' => $days,
            'exempt' => $exempt, 'identity' => $identity, 'insert' => $row,
            'aligned_expiry' => $row === null || $days === -1 ? null
                : \DateTimeImmutable::createFromFormat('!YmdHis', $row['length'], new \DateTimeZone($zone))->getTimestamp()];
    }
}
$json = json_encode(['source_sha256' => $pin,
    'scope' => 'Issuer SQL only. Timezones are synthetic alternatives, not deployment claims. Database active/start defaults and posting decisions are qualified separately.',
    'cases' => $cases], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
if (($argv[3] ?? '') === '--check') {
    if (file_get_contents($argv[2]) !== $json) throw new \RuntimeException('Automatic issuer fixture differs.');
} else file_put_contents($argv[2], $json);
echo 'Automatic issuer fixture matches ' . count($cases) . " source cases.\n";
