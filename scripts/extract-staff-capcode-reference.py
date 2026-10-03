"""Execute selected pinned capcode helpers with synthetic ranks and permissions."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
hashes = {
    "imgboard.php": "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445",
    "lib/auth.php": "98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019",
}
bodies = {}
for relative, expected in hashes.items():
    data = (args.source / relative).read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Audited source changed.")
    bodies[relative] = data.decode("utf-8")
auth = bodies["lib/auth.php"]
helpers = auth[auth.index("function has_level("):auth.index("function is_user()")]
level_map = re.search(r'\$levelorderf\s*=\s*(array\([\s\S]*?\));', auth).group(1)
post = bodies["imgboard.php"]
capcode = post[post.index("function parse_capcode("):post.index("function generate_tim()")]
name_start = post.index("if (!has_level('admin') && !has_flag('capcodename'))")
name_end = post.index("// Only pass and authed users can use VIP capcodes", name_start)
name_rule = post[name_start:name_end]
if len(helpers) > 4096 or len(capcode) > 4096 or len(name_rule) > 256 or len(level_map) > 512:
    raise ValueError("Selected source exceeds audited bounds.")
choices = ["", "capcode_mod", "capcode_dev", "capcode_manager", "capcode_admin",
           "capcode_founder", "capcode_admin_hl", "capcode_unknown"]
rows = []
environment = None
for role in ["janitor", "mod", "manager", "admin"]:
    recipes = []
    for bits in range(8):
        flags = [flag for n, flag in enumerate(["capcode", "developer", "capcodename"]) if bits & (1 << n)]
        for allow_boards in [["all"], ["g"]]:
            for deny_boards in [[], ["noboard"]]:
                for choice in choices:
                    recipes.append({"role": role, "flags": flags, "allow_boards": allow_boards,
                                    "deny_boards": deny_boards, "choice": choice})
    # The null name argument excludes the Pass/VIP branch. The cookie value is
    # synthetic display input to the selected helper, never authentication.
    program = (
        "define('YES',true); define('S_CANTCAPCODE','cant_capcode'); define('S_ANONAME','Anonymous'); "
        "function is_local_auth(){return false;} class OwnedCapcodeStop extends Exception{} "
        "function error($message){throw new OwnedCapcodeStop($message);} "
        "$_COOKIE=['4chan_auser'=>'owned-synthetic']; "
        f"$auth=['level'=>{json.dumps(role)},'guest'=>false,'allow'=>[],'deny'=>[],'flags'=>[]]; $levelorderf={level_map}; "
        + helpers + capcode
        + "$recipes=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR); $rows=[]; "
        "foreach($recipes as $recipe){$auth['flags']=$recipe['flags'];$auth['allow']=$recipe['allow_boards'];$auth['deny']=$recipe['deny_boards']; "
        "$email=$recipe['choice'];$name='Owned finished name'; "
        "if(strpos($email,'capcode_')===0){" + name_rule + "} "
        "try{$result=['outcome'=>parse_capcode($email),'name'=>$name];}catch(OwnedCapcodeStop $e){$result=['outcome'=>$e->getMessage(),'name'=>$name];} "
        "$rows[]=$recipe+$result;} echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
    )
    run = subprocess.run(["php", "-d", "memory_limit=64M", "-d", "max_execution_time=5", "-r", program],
                         input=json.dumps(recipes), text=True, capture_output=True, timeout=10, check=True)
    if run.stderr or len(run.stdout.encode("utf-8")) > 131072:
        raise ValueError("Unexpected pure source output.")
    result = json.loads(run.stdout)
    if len(result["cases"]) != len(recipes) or (environment is not None and result["php"] != environment):
        raise ValueError("Source audit omitted cases or changed runtime.")
    environment = result["php"]
    rows.extend(result["cases"])
fixture = {"reference": "operator-supplied 4chan-old checkout", "files": hashes, "extractor_php": environment,
           "scope": "authenticated public capcode selection and post-preparation name masking; Pass/VIP excluded",
           "boundary_stubs": ["synthetic rank, flags and board scope", "disabled local-auth override",
                              "synthetic username cookie", "terminal error sentinel", "null Pass/VIP name argument"],
           "cases": rows}
data = (json.dumps(fixture, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
if len(rows) != 1024 or len(data) > 524288:
    raise ValueError("Capcode fixture exceeds audited bounds.")
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError("Staff capcode source reference differs.")
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({"cases": len(rows), "outcomes": sorted({row["outcome"] for row in rows})}))
