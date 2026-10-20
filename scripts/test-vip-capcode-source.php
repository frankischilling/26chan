<?php
// Execute only the pinned VIP fragments against owned rows and effect counters.
// No original application, database connection or configuration is loaded.
$source = json_decode(file_get_contents(__DIR__ . '/../fixtures/vip-capcode-source.json'), true, 512, JSON_THROW_ON_ERROR);
eval($source['lookup_php']);
eval($source['dispatch_php']);
$_SERVER['REMOTE_ADDR'] = '192.0.2.19';
$_SERVER['REQUEST_TIME'] = 1700000000;
$owned_hash = password_hash('owned-secret!', PASSWORD_BCRYPT, ['cost' => 4]);
$fixture = [];
$effects = [];

function mysql_global_call($query, ...$args) {
    global $fixture, $effects, $owned_hash;
    if (str_starts_with($query, 'SELECT COUNT(*) FROM user_actions')) {
        if (!str_contains($query, "action = 'fail_login'")
            || !str_contains($query, 'time >= SUBDATE(NOW(), INTERVAL 1 HOUR)')) {
            throw new Exception('Failure accounting query changed');
        }
        $effects['count_reads']++;
        return ['kind' => 'count', 'value' => $fixture['failures']];
    }
    if (str_starts_with($query, 'SELECT name, user_key FROM vip_capcodes')) {
        if (!str_contains($query, 'active = 1')) {
            throw new Exception('Active account gate changed');
        }
        $effects['lookups']++;
        $effects['identifier'] = $args[0];
        return [
            'kind' => 'account',
            'value' => $fixture['active'] && $args[0] === 'owner'
                ? ['name' => 'Owned Verified Name', 'user_key' => $owned_hash]
                : false,
        ];
    }
    if (str_starts_with($query, 'UPDATE vip_capcodes SET last_used')) {
        if ($args !== [1700000000, '192.0.2.19', 'owner']) {
            throw new Exception('Unexpected last-use context');
        }
        $effects['last_used_updates']++;
        return true;
    }
    if (str_starts_with($query, 'INSERT INTO user_actions')) {
        if (!str_contains($query, "'fail_login'")) {
            throw new Exception('Unexpected action');
        }
        $effects['failure_writes']++;
        return true;
    }
    throw new Exception('Unexpected source database operation');
}
function mysql_fetch_row($result) {
    if ($result['kind'] !== 'count') throw new Exception('Wrong count result');
    return [$result['value']];
}
function mysql_fetch_assoc($result) {
    if ($result['kind'] !== 'account') throw new Exception('Wrong account result');
    return $result['value'];
}

// The original list assignment warns for missing components, then rejects their
// null values. Preserve that outcome while rejecting every unrelated warning.
set_error_handler(function ($severity, $message) {
    if (str_starts_with($message, 'Undefined array key')) return true;
    throw new ErrorException($message, 0, $severity);
});
$cases = [];
foreach ([
    'capcode_!owner!owned-secret!',
    'sagecapcode_!owner!owned-secret!',
    'capcode_!owner!owned-SAGEsecret!',
    'capcode_!owner!wrong',
    'capcode_!unknown!owned-secret!',
    'capcode_!owner!',
    'capcode_!owner!0',
    'capcode_!0!owned-secret!',
    'capcode_!!owned-secret!',
    'capcode_!owner',
    'capcode_!owner!!',
    'capcode_!00!00',
    'capcode_! ! ',
    'capcode_!名前!秘密',
    'CAPCODE_!owner!owned-secret!',
    ' capcode_!owner!owned-secret!',
    'capcode_admin',
    '',
] as $raw) {
    foreach ([false, true] as $bypass) {
        foreach ([0, 2, 3, 4] as $failures) {
            foreach ([false, true] as $active) {
                $fixture = ['failures' => $failures, 'active' => $active];
                $effects = [
                    'count_reads' => 0, 'lookups' => 0, 'identifier' => null,
                    'failure_writes' => 0, 'last_used_updates' => 0,
                ];
                $prepared = str_ireplace('sage', '', $raw);
                $name = str_starts_with($prepared, 'capcode_') ? 'Anonymous' : 'Owned Input';
                $capcode = $bypass ? parse_capcode($prepared, $name) : parse_capcode($prepared);
                $cases[] = [
                    'raw' => $raw, 'bypass' => $bypass, 'failures' => $failures,
                    'active' => $active, 'capcode' => $capcode, 'name' => $name,
                    'effects' => $effects,
                ];
            }
        }
    }
}
restore_error_handler();
$encoded = json_encode($cases, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR) . "\n";
$path = __DIR__ . '/../crates/domain/tests/fixtures/vip-capcode.json';
if (in_array('--write', $argv, true)) {
    file_put_contents($path, $encoded);
} elseif (file_get_contents($path) !== $encoded) {
    throw new Exception('VIP source outcomes changed');
}
echo 'VIP source cases passed: ' . count($cases) . "\n";
