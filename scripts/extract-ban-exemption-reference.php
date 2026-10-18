<?php
// Execute only hash-pinned pure CIDR helpers and the synthetic whitelist wrapper.
if ($argc < 3 || $argc > 4 || ($argc === 4 && $argv[3] !== '--check')) {
    fwrite(STDERR, "Usage: php extract-ban-exemption-reference.php SOURCE OUTPUT [--check]\n"); exit(2);
}
$root = rtrim($argv[1], '/');
$hashes = [
    'lib/util.php' => '5307cea255e2d3eb243c7549e1a6521281bf6253c47e010476412b9a4c63a70c',
    'lib/admin.php' => '571d154aa43cd51ee17bb4dfec5502d6dd63c265f4a2526820c011c934c7d185',
];
$source = [];
foreach ($hashes as $path => $hash) {
    $text = file_get_contents("$root/$path");
    if ($text === false || hash('sha256', $text) !== $hash) throw new RuntimeException('Source hash differs');
    $source[$path] = $text;
}
function selected_function($source, $name) {
    if (!preg_match('/function\s+' . preg_quote($name, '/') . '\s*\(/', $source, $match, PREG_OFFSET_CAPTURE)) {
        throw new RuntimeException('Missing selected helper');
    }
    $start = $match[0][1]; $brace = strpos($source, '{', $start); $depth = 0;
    // These three pinned bodies have no braces inside strings or comments.
    for ($end = $brace; $end < strlen($source) && $end - $start < 2048; $end++) {
        if ($source[$end] === '{') $depth++;
        if ($source[$end] === '}' && --$depth === 0) return substr($source, $start, $end-$start+1);
    }
    throw new RuntimeException('Selected helper exceeded its boundary');
}
function post_filter_get($name) {
    if ($name !== 'ipwhitelist') throw new RuntimeException('Unexpected list');
    return [$GLOBALS['synthetic_ranges'], []];
}
eval(selected_function($source['lib/util.php'], 'cidrtest'));
eval(selected_function($source['lib/util.php'], 'find_ipxff_in'));
eval(selected_function($source['lib/admin.php'], 'whitelisted_ip'));
$configs = ['' => null];
foreach (range(1, 32) as $prefix) foreach ([0xc0000200,0xc6336400,0xcb0071ff,0x80000000] as $seed) {
    $mask = (0xffffffff << (32-$prefix)) & 0xffffffff;
    $base = $seed & $mask;
    if ($base === 0) continue;
    $configs[long2ip($base)."/$prefix"] = [$base,$prefix];
}
$cases = [];
foreach ($configs as $cidr => $network) {
    if ($network === null) $peers = [0,1,0xc0000201,0xffffffff];
    else {
        [$base,$prefix] = $network; $last = $base | ((1 << (32-$prefix))-1);
        $peers = array_values(array_unique([0,0xffffffff,$base,$last,max(0,$base-1),min(0xffffffff,$last+1),min($last,$base+1),max($base,$last-1)]));
    }
    $GLOBALS['synthetic_ranges'] = $network === null ? [] : ['# Synthetic fixture', $cidr];
    foreach ($peers as $peer) {
        $_SERVER['REMOTE_ADDR'] = long2ip($peer);
        $cases[] = ['network'=>$network === null ? null : $cidr, 'peer'=>long2ip($peer), 'exempt'=>whitelisted_ip()];
    }
}
$out = "{\n  \"source_sha256\": ".json_encode($hashes,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR).",\n  \"cases\": [\n";
$out .= implode(",\n",array_map(fn($case)=>'    '.json_encode($case,JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),$cases));
$out .= "\n  ]\n}\n";
if ($argc === 4) {
    if (file_get_contents($argv[2]) !== $out) throw new RuntimeException('Fixture differs');
    echo 'Ban exemption fixture matches '.count($cases)." cases.\n";
} else {
    if (file_put_contents($argv[2],$out) !== strlen($out)) throw new RuntimeException('Incomplete fixture write');
    echo 'Wrote '.count($cases)." ban exemption cases.\n";
}
