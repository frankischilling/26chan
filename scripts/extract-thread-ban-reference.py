"""Execute unchanged pinned cpban/ban_post with owned SQL, DNS and RPC stubs."""
import argparse
import hashlib
import itertools
import json
import subprocess
from pathlib import Path

PIN = "4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb"
NOW = 1700000000


def recipes():
    graphs = [[], [(101, "198.51.100.1")],
              [(102, "198.51.100.2"), (103, "198.51.100.2")],
              [(101, "198.51.100.1"), (102, "198.51.100.2"),
               (103, "198.51.100.2"), (104, "198.51.100.3")]]
    cases = []
    for graph, op_days, rep_days in itertools.product(graphs, [0, 1, 3, 9999], [0, 1, 3, 9999]):
        cases.append(dict(no=100, op_reason='OP <synthetic> & "quoted"',
                          rep_reason="Reply 'synthetic' & reason", op_days=op_days,
                          rep_days=rep_days, posts=[dict(no=100, host="198.51.100.1", resto=0)]
                          + [dict(no=no, host=host, resto=100) for no, host in graph]))
    base = cases[-1]
    for field, value in [("no", 0), ("no", 999), ("no", 102),
                         ("op_reason", ""), ("op_reason", "0"),
                         ("rep_reason", ""), ("rep_reason", "0"),
                         ("op_days", -1), ("op_days", 10000),
                         ("rep_days", -1), ("rep_days", 10000)]:
        cases.append({**base, field: value})
    for i, case in enumerate(cases):
        case["name"] = f"thread-{i}"
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = (args.source / "admin.php").read_bytes()
    assert hashlib.sha256(data).hexdigest() == PIN, "Audited thread-ban source changed"
    source = data.decode().replace("\r\n", "\n")
    functions = source[source.index("function ban_post("):source.index("function delete_post(")]
    assert functions.count("function ") == 2
    program = r'''
date_default_timezone_set('UTC');
define('SQLLOG','test'); define('SQLLOGBAN','banned_users'); define('BOARD_DIR','test');
define('S_SQLFAIL','Synthetic SQL failure');
function time() { return 1700000000; }
function gethostbyaddr($host) { return $host; }
class Rows { public $index=0; function __construct(public $rows) {} }
$case=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
$bans=[]; $deletions=[];
$_POST=$case; $_COOKIE=['4chan_auser'=>'synthetic-moderator'];
$_SERVER=['REMOTE_ADDR'=>'192.0.2.1'];
function mysql_board_call($sql) {
 global $case;
 if (preg_match('/^SELECT HIGH_PRIORITY \* FROM `test` WHERE no=(-?[0-9]+)$/',$sql,$m)) {
  $rows=array_values(array_filter($case['posts'],fn($p)=>$p['no']===(int)$m[1]));
 } elseif (preg_match("/^SELECT no, host FROM `test` WHERE resto = (-?[0-9]+) AND host != '([^']+)' GROUP BY host$/",$sql,$m)) {
  $group=[];
  foreach ($case['posts'] as $p) if ($p['resto']===(int)$m[1] && $p['host']!==$m[2] && !isset($group[$p['host']])) $group[$p['host']]=$p;
  $rows=array_values($group);
 } else throw new Exception('Unexpected board query');
 return new Rows(array_map(fn($p)=>$p+['name'=>'Synthetic name','md5'=>'synthetic-md5',
  '4pass_id'=>'synthetic-pass-'.$p['no'],'time'=>1700000000,'pwd'=>'synthetic-password'],$rows));
}
function mysql_fetch_assoc($rows) { return $rows->rows[$rows->index++] ?? false; }
function make_post_json($row) { return json_encode(['no'=>$row['no']],JSON_THROW_ON_ERROR); }
function mysql_global_do($sql,...$args) {
 global $bans;
 if (!preg_match('/INSERT INTO banned_users\s*\(([^)]+)\)/s',$sql,$m)) throw new Exception('Unexpected ban query');
 $columns=array_map('trim',explode(',',$m[1]));
 if (count($columns)!==count($args)) throw new Exception('Ban projection changed');
 $bans[]=array_combine($columns,$args); return true;
}
function delete_post($no,$imgonly,$template=null,$tool=null) {
 global $deletions; $deletions[]=[$no,$imgonly,$template,$tool]; return true;
}
ob_start();
register_shutdown_function(function() {
 global $bans,$deletions;
 $output=ob_get_clean();
 echo json_encode(['php'=>PHP_VERSION,'bans'=>$bans,'deletions'=>$deletions,'output'=>$output],JSON_THROW_ON_ERROR);
});
'''
    program += functions + "\ncpban($case['no']);\n"
    cases = recipes()
    version = None
    for case in cases:
        result = subprocess.run(["php", "-n", "-d", "disable_functions=time,gethostbyaddr",
                                 "-d", "memory_limit=64M", "-d", "max_execution_time=2", "-r", program],
                                input=json.dumps(case).encode(), capture_output=True, timeout=5, check=True)
        assert not result.stderr and len(result.stdout) < 20_000
        observed = json.loads(result.stdout)
        version = observed.pop("php")
        case["observed"] = observed
    fixture = dict(reference="operator-supplied 4chan-old checkout",
                   source_revision="545b7812d1849f7958d914950c91fdbbe38f6b22",
                   files={"admin.php": PIN}, extractor_php=version, request_at=NOW,
                   scope="Unchanged cpban and ban_post: validation, OP/reply grouping, global fields, reasons and day arithmetic",
                   boundary_stubs=["owned synthetic board rows and one selected row per address group",
                                   "recorded global inserts without a real database",
                                   "fixed clock and UTC; disabled/replaced DNS lookup",
                                   "synthetic saved-post JSON; recorded deletion RPC without network"],
                   excludes=["source database defaults and Pass authentication", "SQL representative ordering",
                             "staff authentication/CSRF and source form presentation", "actual deletion RPC or transaction semantics"],
                   cases=cases)
    encoded = json.dumps(fixture, ensure_ascii=False, separators=(",", ":")) + "\n"
    if args.check:
        assert args.output.read_text(encoding="utf-8") == encoded, "Recorded thread reference changed"
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(f"{len(cases)} thread-ban source traces verified with PHP {version}.")


if __name__ == "__main__":
    main()
