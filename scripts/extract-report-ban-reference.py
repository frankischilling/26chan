"""Execute pinned report_check_ip with isolated synthetic ban matches."""
import argparse
import hashlib
import itertools
import json
import subprocess
from pathlib import Path


PIN = "e52c0042295c942c12bfbfa404ca02b14932309fecd8aa9c2b3cc298aefa91ae"


def recipes():
    cases = []
    for key, kind, scope, active, check, present, bypass, password, template in itertools.product(
        ["host", "password", "4pass_id"],
        ["finite", "expired", "boundary", "permanent", "warning", "negative_warning"],
        ["local", "other", "global"],
        [False, True], [False, True], [False, True], [False, True], [False, True], [1, 124],
    ):
        start = 1_699_999_000
        end = {"finite": 1_700_000_001, "expired": 1_699_999_999,
               "boundary": 1_700_000_000, "permanent": 0,
               "warning": start, "negative_warning": start - 1}[kind]
        cases.append(dict(name=f"matrix-{len(cases)}", check_ban=check, pass_present=present,
                          bypass=bypass, password_present=password, bans=[dict(
                              key=key, active=active, board="other" if scope == "other" else "test",
                              **{"global": int(scope == "global")}, starts_on=start, ends_on=end,
                              template_id=template, admin="Auto-ban", reason="Proxy synthetic")]))
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data = (args.source / "modes/report.php").read_bytes()
    assert hashlib.sha256(data).hexdigest() == PIN, "Audited report source changed"
    source = data.decode().replace("\r\n", "\n")
    start = source.index("  function report_check_ip(")
    end = source.index("\n\tfunction report_increment_counter()", start)
    function = source[start:end].strip()
    assert function.endswith("}") and len(function) < 6000
    program = r'''
define('BOARD_DIR','test'); define('RENZOKU_REP_HOURLY',100); define('RENZOKU_REP_DAILY',100);
class L { static function d($board) { return 'example.invalid'; } }
class UserPwd {
  static function getSession() { global $case; return $case['password_present'] ? new self() : null; }
  function getPwd() { return 'synthetic-password'; }
}
class SourceRefusal extends Exception {}
function fancydie($message) { throw new SourceRefusal($message); }
function mysql_real_escape_string($value) { return addslashes($value); }
function mysql_num_rows($rows) { return count($rows); }
function mysql_fetch_row($rows) { return $rows[0] ?? false; }
function isIPRangeBannedReport($ip,$asn,$board,$pwd) { return false; }
function mysql_global_call($sql) {
  global $case;
  if (str_starts_with($sql,'SELECT no FROM reports WHERE ')) return [];
  if (str_starts_with($sql,'SELECT COUNT(*) FROM reports WHERE ')) return [[0]];
  if (!str_starts_with($sql,'SELECT COUNT(*) FROM banned_users WHERE ')
      || !str_ends_with($sql," AND active = 1 AND (global = 1 OR board = 'test')")) {
    throw new Exception('Unexpected source query');
  }
  preg_match_all("/(host|password|4pass_id) = '[^']+'/", $sql, $matches);
  $count=0;
  foreach ($case['bans'] as $ban) {
    if ($ban['active'] && in_array($ban['key'],$matches[1],true)
        && ($ban['global'] || $ban['board']==='test')) $count++;
  }
  return [[$count]];
}
'''
    program += function + r'''
$cases=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR); $results=[];
foreach ($cases as $case) {
  $captcha_bypass=$case['bypass']; $passid=$case['pass_present'] ? 'synthetic-pass' : null;
  $_SERVER=['REMOTE_ADDR'=>'198.51.100.3','REQUEST_TIME'=>1700000000,'HTTP_X_GEO_ASN'=>'0'];
  try { report_check_ip('test','100',$case['check_ban']); $blocked=false; }
  catch (SourceRefusal $e) {
    if ($e->getMessage()!=="You can't report posts because you are <a href=\"https://www.example.invalid/banned\" target=\"_blank\">banned</a>.") throw $e;
    $blocked=true;
  }
  $results[]=$blocked;
}
echo json_encode(['php'=>PHP_VERSION,'results'=>$results],JSON_THROW_ON_ERROR);
'''
    cases = recipes()
    result = subprocess.run(["php", "-n", "-d", "memory_limit=128M", "-d", "max_execution_time=15", "-r", program],
                            input=json.dumps(cases).encode(), capture_output=True, timeout=20, check=True)
    assert not result.stderr and len(result.stdout) < 100_000
    observed = json.loads(result.stdout)
    assert len(observed["results"]) == len(cases)
    for case, blocked in zip(cases, observed["results"], strict=True):
        case["blocked"] = blocked
    fixture = dict(reference="operator-supplied 4chan-old checkout",
                   source_revision="545b7812d1849f7958d914950c91fdbbe38f6b22",
                   files={"modes/report.php": PIN}, extractor_php=observed["php"],
                   scope="Executed report_check_ip ban block: active, scope, identity clause selection; GET and POST",
                   boundary_stubs=["zero prior reports and quotas", "synthetic password and server-qualified Pass/bypass",
                                   "active exact-identity synthetic database count", "range restriction returns false",
                                   "fixed request time and synthetic domain"],
                   excludes=["range/ASN restrictions", "Pass authentication", "quotas and target policy",
                             "database identity binding, privileges and transactions", "original page rendering"],
                   cases=cases)
    encoded = json.dumps(fixture, ensure_ascii=False, separators=(",", ":")) + "\n"
    if args.check:
        assert args.output.read_text(encoding="utf-8") == encoded, "Recorded report comparison changed"
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(encoded, encoding="utf-8")
    print(f"{len(cases)} source report-ban decisions verified with PHP {observed['php']}.")


if __name__ == "__main__":
    main()
