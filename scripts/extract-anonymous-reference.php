<?php
// Execute only named, hash-pinned activity methods on synthetic state. Do not
// evaluate the original class, constructor, cookie codec or cryptography.
if ($argc < 3) {
    fwrite(STDERR, "Usage: php scripts/extract-anonymous-reference.php SOURCE OUTPUT [--check]\n");
    exit(2);
}
$root = realpath($argv[1]);
if (!$root) { throw new RuntimeException('Reference directory is missing.'); }
$source = file_get_contents($root . '/lib/userpwd.php');
$hash = '0a753a44e091aeb23b0a99fa09be6c1d513c8163f85989c8d4dedf7949e1bc26';
if (hash('sha256', $source) !== $hash) {
    throw new RuntimeException('Audited anonymous-session source differs.');
}
$fields = ['creation_ts', 'mask_ts', 'ip_ts', 'env_ts', 'activity_ts', 'action_ts',
    'verified_level', 'post_count', 'img_count', 'thread_count', 'report_count',
    'action_buffer', 'ip_change_score'];
$constants = ['TTL', 'ACTION_DELAY', 'IP_CHANGE_DELAY', 'IP_CHANGE_SCORE_MAX',
    'IP_CHANGE_MASK_VAL', 'IP_CHANGE_IP_VAL', 'A_POST', 'A_IMG', 'A_THREAD', 'A_REPORT'];
$methods = ['pwdLifetime', 'maskLifetime', 'ipLifetime', 'envLifetime', 'idleLifetime',
    'lastActionLifetime', 'maskChanged', 'ipChanged', 'envChanged', 'isNew',
    'verifiedLevel', 'isUserKnown', 'isUserKnownOrVerified', 'updatePostActivity',
    'updateReportActivity', 'updateActivity', 'postCount', 'imgCount', 'threadCount',
    'reportCount', 'ipChangeScore', 'resetTimestamps', 'resetActionCounts'];
$class = 'class AnonymousReference { public $now = 0; public $pwd_sig = null;';
foreach ($fields as $field) { $class .= 'public $' . $field . ' = 0;'; }
foreach ($constants as $constant) {
    if (!preg_match('/const\s+' . $constant . '\s*=\s*([0-9]+)\s*;/', $source, $match)) {
        throw new RuntimeException('Audited numeric constant is missing.');
    }
    $class .= 'const ' . $constant . '=' . $match[1] . ';';
}
$found = [];
$tokens = token_get_all($source);
for ($i = 0; $i < count($tokens); ++$i) {
    if (!is_array($tokens[$i]) || $tokens[$i][0] !== T_FUNCTION) { continue; }
    $start = $i;
    do { ++$i; } while (is_array($tokens[$i]) && $tokens[$i][0] === T_WHITESPACE);
    if (!is_array($tokens[$i]) || !in_array($tokens[$i][1], $methods, true)) { continue; }
    $name = $tokens[$i][1];
    if (isset($found[$name])) { throw new RuntimeException('Repeated audited method.'); }
    $depth = 0; $opened = false; $body = '';
    for ($j = $start; $j < count($tokens); ++$j) {
        $token = $tokens[$j]; $body .= is_array($token) ? $token[1] : $token;
        if ($token === '{') { ++$depth; $opened = true; }
        if ($token === '}' && --$depth === 0 && $opened) { break; }
    }
    if ($depth !== 0 || !$opened || strlen($body) > 4096) {
        throw new RuntimeException('Audited method exceeds its bounds.');
    }
    $class .= $body;
    $found[$name] = true;
}
if (count($found) !== count($methods) || strlen($class) > 16384) {
    throw new RuntimeException('Audited method set is incomplete.');
}
eval($class . '}');
function reference_state($now, $values = []) {
    $state = new AnonymousReference(); $state->now = $now;
    $state->creation_ts = $state->mask_ts = $state->ip_ts = $state->env_ts = $now;
    foreach ($values as $key => $value) { $state->$key = $value; }
    return $state;
}
function saved_state($state, $fields) {
    $result = [];
    foreach ($fields as $field) { $result[$field] = $state->$field; }
    return $result;
}
$now = 1700000000;
$known = [];
foreach ([0, 1799, 1800, 86399, 86400, 86401] as $network_age) {
    foreach ([86399, 86400, 86401] as $password_age) {
        foreach ([9, 10] as $score) {
            foreach ([[0, 0], [2, 9], [3, 10], [8, 19], [9, 20], [11, 6]] as [$posts, $reports]) {
                foreach ([0, $now - 1800, $now] as $since) {
                    foreach ([0, 1] as $verified) {
                        $state = reference_state($now, ['creation_ts' => $now - $password_age,
                            'mask_ts' => $now - $network_age, 'ip_change_score' => $score,
                            'post_count' => $posts, 'report_count' => $reports,
                            'verified_level' => $verified]);
                        $known[] = ['password_age' => $password_age, 'network_age' => $network_age,
                            'score' => $score, 'posts' => $posts, 'reports' => $reports,
                            'pending' => 0, 'verified' => $verified, 'minutes' => 1440,
                            'since' => $since, 'known' => $state->isUserKnown(1440, $since),
                            'known_or_verified' => $state->isUserKnownOrVerified(1440, $since)];
                    }
                }
            }
        }
    }
}
foreach ([0, 1, 8, 15] as $pending) {
    foreach ([[2, 9], [8, 19], [0, 5], [255, 255]] as [$posts, $reports]) {
        foreach ([0, $now] as $since) {
            $state = reference_state($now, ['creation_ts' => $now - 86400,
                'post_count' => $posts, 'report_count' => $reports, 'action_buffer' => $pending]);
            $known[] = ['password_age' => 86400, 'network_age' => 0, 'score' => 0,
                'posts' => $posts, 'reports' => $reports, 'pending' => $pending, 'verified' => 0,
                'minutes' => 1440, 'since' => $since, 'known' => $state->isUserKnown(1440, $since),
                'known_or_verified' => $state->isUserKnownOrVerified(1440, $since)];
        }
    }
}
foreach ([0, 15, 60, 120, 240, 360, 4320] as $minutes) {
    foreach ([max(0, $minutes * 60 - 1), $minutes * 60, $minutes * 60 + 1] as $network_age) {
        foreach ([9, 10] as $score) {
            $state = reference_state($now, ['creation_ts' => $now - $minutes * 60,
                'mask_ts' => $now - $network_age, 'ip_change_score' => $score]);
            $known[] = ['password_age' => $minutes * 60, 'network_age' => $network_age,
                'score' => $score, 'posts' => 0, 'reports' => 0, 'pending' => 0, 'verified' => 0,
                'minutes' => $minutes, 'since' => 0, 'known' => $state->isUserKnown($minutes),
                'known_or_verified' => $state->isUserKnownOrVerified($minutes)];
        }
    }
}
$activities = [];
foreach ([0, 14399, 14400, 14401] as $elapsed) {
    foreach ([1, 3, 5, 7, 8] as $kind) {
        foreach ([false, true] as $dummy) {
            foreach ([0, 29, 31, 32] as $score) {
                foreach (['stable', 'address', 'network'] as $change) {
                    foreach ([1799, 1800] as $idle) {
                        $state = reference_state($now, ['creation_ts' => $now - 86400,
                            'mask_ts' => $change === 'network' ? $now : $now - 3600,
                            'ip_ts' => $change !== 'stable' ? $now : $now - 3600,
                            'activity_ts' => $now - $idle, 'action_ts' => $now - $elapsed,
                            'ip_change_score' => $score, 'post_count' => 255, 'img_count' => 3,
                            'thread_count' => 7, 'report_count' => 255, 'action_buffer' => 9]);
                        $before = saved_state($state, $fields);
                        $state->updateActivity($kind, $dummy);
                        $activities[] = ['now' => $now, 'kind' => $kind, 'dummy' => $dummy,
                            'before' => $before, 'after' => saved_state($state, $fields),
                            'counts' => [$state->postCount(), $state->imgCount(),
                                $state->threadCount(), $state->reportCount()]];
                    }
                }
            }
        }
    }
}
foreach ([false, true] as $fresh) {
    foreach ([1, 7, 8] as $kind) {
        $state = reference_state($now, ['creation_ts' => $fresh ? $now : $now - 86400,
            'ip_change_score' => 10]);
        $before = saved_state($state, $fields); $state->updateActivity($kind);
        $activities[] = ['now' => $now, 'kind' => $kind, 'dummy' => false,
            'before' => $before, 'after' => saved_state($state, $fields),
            'counts' => [$state->postCount(), $state->imgCount(),
                $state->threadCount(), $state->reportCount()]];
    }
}
$resets = [];
foreach ([0, $now - 604799, $now - 604800, $now - 604801] as $last_activity) {
    foreach ([604799, 604800, 604801] as $created_age) {
        $state = reference_state($now, ['creation_ts' => $now - $created_age,
            'activity_ts' => $last_activity, 'action_ts' => $now - 86400,
            'post_count' => 19, 'report_count' => 7, 'action_buffer' => 15,
            'verified_level' => 1, 'ip_change_score' => 12]);
        $before = saved_state($state, $fields);
        $last = $last_activity > 0 ? $last_activity : $state->creation_ts;
        // The pinned constructor assigns the retained password first, then
        // returns from this TTL branch before restoring counters or verification.
        $expired = $now - $last >= AnonymousReference::TTL;
        if ($expired) { $state = reference_state($now); $state->resetTimestamps(); }
        $resets[] = ['now' => $now, 'before' => $before, 'expired' => $expired,
            'after' => saved_state($state, $fields)];
    }
}
$metadata = ['reference' => 'operator-supplied 4chan-old checkout', 'extractor_php' => PHP_VERSION,
    'files' => ['lib/userpwd.php' => $hash], 'now' => $now,
    'methods' => $methods, 'known_cases' => [], 'activity_cases' => [], 'reset_cases' => []];
$json = json_encode($metadata, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR);
foreach (['known_cases' => $known, 'activity_cases' => $activities, 'reset_cases' => $resets] as $key => $cases) {
    $rows = array_map(fn($case) => '        ' . json_encode($case, JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR), $cases);
    $json = str_replace('"' . $key . '": []', '"' . $key . '": [' . "\n" . implode(",\n", $rows) . "\n    ]", $json);
}
$json .= "\n";
if (strlen($json) > 4 * 1024 * 1024 || count($known) !== 1370 || count($activities) !== 966) {
    throw new RuntimeException('Anonymous reference exceeds its fixed bounds.');
}
if (in_array('--check', $argv, true)) {
    if (file_get_contents($argv[2]) !== $json) { throw new RuntimeException('Anonymous reference differs.'); }
    echo 'Anonymous reference matches ' . count($known) . ' known-user, ' . count($activities)
        . ' activity and ' . count($resets) . " idle-reset cases.\n";
} else {
    file_put_contents($argv[2], $json);
}
