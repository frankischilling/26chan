"""Execute the pinned posting ban decision against synthetic database rows."""
import argparse
import hashlib
import itertools
import json
import subprocess
import tempfile
from pathlib import Path


PIN = "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445"
NOW = 1_700_000_000


def ban(key="host", *, kind="finite", scope="local", template=1,
        automatic=False, reason="Synthetic reason", active=True):
    start = NOW - 1000
    end = {"finite": NOW + 1, "boundary": NOW, "expired": NOW - 1,
           "permanent": 0, "warning": start, "negative_warning": start - 1}[kind]
    return dict(key=key, no=1, active=active,
                board="g" if scope == "other" else "test",
                **{"global": int(scope == "global"), "4pass_id": "synthetic-pass"},
                post_num=100, template_id=template, admin="Auto-ban" if automatic else "Synthetic moderator",
                reason=reason, starts_on=start, ends_on=end)


def recipes():
    rows = []
    combinations = itertools.product(
        ["host", "password", "4pass_id"],
        ["finite", "boundary", "expired", "permanent", "warning", "negative_warning"],
        ["local", "other", "global"], [False, True], [False, True],
        [False, True], [1, 124], [False, True],
        ["Synthetic reason", "Proxy synthetic", "proxy synthetic", "Private<>Proxy synthetic"],
        [False, True],
    )
    for key, kind, scope, bypass, pass_present, verified, template, automatic, reason, active in combinations:
        rows.append(dict(name=f"matrix-{len(rows)}", bypass=bypass, pass_present=pass_present,
                         verified=verified, field_order=["password", "4pass_id"],
                         bans=[ban(key, kind=kind, scope=scope, template=template,
                                   automatic=automatic, reason=reason, active=active)]))
    # Explicit ordering: the source stops scanning on the first warning or ban.
    sequences = [
        [ban(kind="expired"), ban(kind="finite")],
        [ban(kind="finite"), ban(kind="expired")],
        [ban(kind="expired"), ban(kind="warning")],
        [ban(kind="warning", scope="other"), ban(kind="expired")],
        [ban("password", scope="other"), ban(kind="finite")],
        [ban("password", kind="expired"), ban("4pass_id", kind="finite"), ban(kind="warning")],
        [ban("4pass_id", template=124), ban(kind="finite")],
        [ban("4pass_id", template=124, kind="warning"), ban(kind="finite")],
    ]
    for index, bans in enumerate(sequences):
        for order in [["password", "4pass_id"], ["4pass_id", "password"]]:
            rows.append(dict(name=f"order-{index}-{'-'.join(order)}", bypass=False,
                             pass_present=True, verified=False, field_order=order, bans=bans))
    for row in rows:
        for index, value in enumerate(row["bans"], 1):
            value["no"] = index
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = (args.source / "imgboard.php").read_bytes()
    assert hashlib.sha256(data).hexdigest() == PIN, "Audited posting source changed"
    source = data.decode().replace("\r\n", "\n")
    begin = source.index("function check_for_ban($ip,")
    finish = source.index("\n/**\n * Strips tags from webms", begin)
    function = source[begin:finish].strip()
    assert function.endswith("}") and len(function) < 4000
    program = r'''
define('BOARD_DIR', 'test');
define('SALTFILE', '/synthetic-salt-not-read');
// A fresh owned empty directory confines the source's thumbnail cleanup.
define('BANTHUMB_ROOT', $argv[1] . '/');
class Rows { public $rows; public $index=0; function __construct($rows) { $this->rows=$rows; } }
function mysql_global_call($sql, $value) {
    global $case;
    if (!preg_match("/AND ([a-zA-Z0-9_]+) = '%s'/", $sql, $match)) throw new Exception('Unexpected source query');
    $key=$match[1];
    return new Rows(array_values(array_filter($case['bans'], function($ban) use ($key) {
        return $ban['active'] && $ban['key'] === $key;
    })));
}
function mysql_num_rows($result) { return count($result->rows); }
function mysql_fetch_assoc($result) { return $result->rows[$result->index++] ?? false; }
function file_get_contents_cached($path) { return 'synthetic-salt'; }
function mysql_global_do($sql) {
    global $expired;
    if (!preg_match('/WHERE no IN\(([0-9,]+)\) LIMIT ([0-9]+)/', $sql, $match)) throw new Exception('Unexpected expiry query');
    $expired=array_map('intval', explode(',', $match[1]));
    if (count($expired)!==(int)$match[2]) throw new Exception('Expiry cardinality');
    return true;
}
'''
    program += function + r'''
$cases=json_decode(stream_get_contents(STDIN), true, 32, JSON_THROW_ON_ERROR);
$results=[];
foreach ($cases as $case) {
    $expired=[]; $captcha_bypass=$case['bypass']; $_SERVER['REQUEST_TIME']=1700000000;
    $fields=[];
    foreach($case['field_order'] as $key) {
        if ($key==='4pass_id' && !$case['pass_present']) continue;
        $fields[$key]=$key==='4pass_id'?'synthetic-pass':'synthetic-password';
    }
    $decision=check_for_ban('198.51.100.3', $fields, 123, $case['verified']);
    $results[]=['decision'=>$decision, 'expired'=>$expired];
}
echo json_encode(['php'=>PHP_VERSION,'results'=>$results], JSON_THROW_ON_ERROR);
'''
    cases = recipes()
    with tempfile.TemporaryDirectory(prefix="26chan-ban-reference-") as owned:
        assert not list(Path(owned).iterdir())
        result = subprocess.run(
            ["php", "-d", "memory_limit=128M", "-d", "max_execution_time=15", "-r", program, owned],
            input=json.dumps(cases).encode(), capture_output=True, timeout=20, check=True,
        )
    assert not result.stderr and len(result.stdout) < 1_000_000
    observed = json.loads(result.stdout)
    assert len(observed["results"]) == len(cases)
    for case, outcome in zip(cases, observed["results"], strict=True):
        case.update(outcome)
    fixture = dict(
        reference="operator-supplied 4chan-old checkout",
        source_revision="545b7812d1849f7958d914950c91fdbbe38f6b22",
        files={"imgboard.php": PIN}, extractor_php=observed["php"],
        scope="Executed check_for_ban: ordered matched fields/rows, warnings, expiry, scopes, Pass and verified proxy exceptions",
        boundary_stubs=["active exact-identity synthetic database matches in prescribed order",
                        "fixed request time and synthetic salt", "expiry UPDATE recorded without a database",
                        "fresh owned empty thumbnail directory", "server-qualified Pass/bypass/verification booleans"],
        excludes=["real identity matching, privileges and transactions", "complete posting endpoint",
                  "Pass authentication and template contents", "warning acknowledgement and public banned controller",
                  "network/range restrictions and janitor requests", "original page rendering"],
        cases=cases,
    )
    encoded = json.dumps(fixture, ensure_ascii=False, separators=(",", ":")) + "\n"
    if args.check:
        assert args.output.read_text(encoding="utf-8") == encoded, "Recorded source comparison changed"
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(f"{len(cases)} source ban decisions verified with PHP {observed['php']}.")


if __name__ == "__main__":
    main()
