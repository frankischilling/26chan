"""Execute pinned staff spoiler helpers against synthetic database boundaries."""
import argparse
import hashlib
import itertools
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
    "admin.php": "4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb",
    "lib/auth.php": "98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019",
}
bodies = {}
for name, expected in hashes.items():
    data = (args.source / name).read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Audited source changed.")
    bodies[name] = data.decode("utf8").replace("\r\n", "\n")
admin, auth = bodies["admin.php"], bodies["lib/auth.php"]
helper = admin[admin.index("function adminToggleSpoiler("):admin.index("function adminopt()")]
endpoint = admin[admin.index("function admin_toggle_spoiler()"):admin.index("function validate_csrf(")]
permissions = auth[auth.index("function has_level("):auth.index("function is_user()")]
level_map = re.search(r"\$levelorderf\s*=\s*(array\([\s\S]*?\));", auth).group(1)
if len(helper) > 2048 or len(endpoint) > 2048 or len(permissions) > 4096 or len(level_map) > 512:
    raise ValueError("Selected source exceeds audited bounds.")
recipes = []
for role, scoped, enabled, old, desired, archived, reply in itertools.product(
        ["janitor", "mod", "manager", "admin"], *([[False, True]] * 6)):
    recipes.append(dict(role=role, scoped=scoped, enabled=enabled, old=old,
                        desired=desired, archived=archived, reply=reply,
                        missing_post=False, missing_flag=False))
for role, missing_post, missing_flag in itertools.product(
        ["janitor", "mod", "manager", "admin"], *([[False, True]] * 2)):
    recipes.append(dict(role=role, scoped=True, enabled=True, old=False,
                        desired=True, archived=False, reply=False,
                        missing_post=missing_post, missing_flag=missing_flag))
rows, environment = [], None
for recipe in recipes:
    # Each process invokes the exact endpoint, including die(), and captures its
    # response at shutdown. No source function is rewritten or given a real DB.
    program = (
        "$recipe=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);"
        "define('BOARD_DIR','g');define('SQLLOG','g');define('SPOILERS',$recipe['enabled']);"
        "$_COOKIE=['4chan_auser'=>'owned-synthetic'];"
        "$_GET=['pid'=>40,'flag'=>$recipe['desired']?'1':'0'];"
        "if($recipe['missing_flag'])unset($_GET['flag']);"
        "$auth=['level'=>$recipe['role'],'guest'=>false,'allow'=>$recipe['scoped']?['g']:['a'],'deny'=>[],'flags'=>[]];"
        f"$levelorderf={level_map};"
        "function is_local_auth(){return false;}function auth_user(){}"
        "$post=['no'=>40,'resto'=>$recipe['reply']?30:0,'archived'=>$recipe['archived'],"
        "'sub'=>($recipe['old']?'SPOILER<>':'').'Owned subject','name'=>'Owned name',"
        "'com'=>'Owned comment','filename'=>'owned','ext'=>'.png'];"
        "$updates=[];$audits=[];$rebuilds=[];"
        "function mysql_board_call($query,...$args){global $updates;"
        "if(strpos($query,'SELECT')===0)return true;$updates[]=$args;return true;}"
        "function mysql_fetch_assoc($result){global $post,$recipe;return $recipe['missing_post']?false:$post;}"
        "function mysql_global_call($query,...$args){global $audits;$audits[]=$args;return true;}"
        "function rebuild_thread($thread,&$error,$archived){global $rebuilds;$rebuilds[]=[$thread,$archived];}"
        + permissions + helper + endpoint
        + "ob_start();register_shutdown_function(function(){global $updates,$audits,$rebuilds;"
        "$response=ob_get_clean();echo json_encode(['php'=>PHP_VERSION,'response'=>$response,"
        "'updates'=>$updates,'audits'=>$audits,'rebuilds'=>$rebuilds],JSON_THROW_ON_ERROR);});"
        "admin_toggle_spoiler();"
    )
    run = subprocess.run(["php", "-d", "memory_limit=64M", "-d", "max_execution_time=5", "-r", program],
                         input=json.dumps(recipe).encode(), capture_output=True, timeout=10, check=True)
    if run.stderr or len(run.stdout) > 4096:
        raise ValueError("Unexpected source endpoint output.")
    result = json.loads(run.stdout)
    if environment is not None and result["php"] != environment:
        raise ValueError("Source runtime changed.")
    environment = result.pop("php")
    rows.append(recipe | result)
fixture = {
    "reference": "operator-supplied 4chan-old checkout",
    "source_revision": "545b7812d1849f7958d914950c91fdbbe38f6b22",
    "files": hashes, "extractor_php": environment,
    "scope": "spoiler endpoint policy, rank/scope checks, state changes, unchanged requests, missing inputs/posts, live/archive rebuilds and action masks",
    "boundary_stubs": ["synthetic rank and board scope", "disabled local authentication",
                       "synthetic GET and username cookie", "successful synthetic SELECT/UPDATE/audit calls",
                       "synthetic post row or absence", "recorded thread rebuild arguments"],
    "cases": rows,
}
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + "\n").encode("utf8")
if len(rows) != 272 or len(data) > 262144:
    raise ValueError("Spoiler fixture exceeds audited bounds.")
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError("Staff spoiler source reference differs.")
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({"cases": len(rows), "changes": sum(bool(row["updates"]) for row in rows)}))
