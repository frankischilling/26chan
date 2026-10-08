"""Extract pinned input and line checks using bounded synthetic staff ranks."""
import argparse
import hashlib
import importlib.util
import json
import re
import subprocess
from pathlib import Path

project = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("source", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--migration", type=Path)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
source = args.source.resolve()
hashes = {
    "imgboard.php": "caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445",
    "lib/auth.php": "98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019",
}
bodies = {}
for relative, expected in hashes.items():
    data = (source / relative).read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("Audited source changed.")
    bodies[relative] = data.decode("utf-8")
spec = importlib.util.spec_from_file_location("pinned_boards", project / "scripts/extract-trip-policy-reference.py")
pin = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pin)
pinned_policy = pin.extract(source)
spec = importlib.util.spec_from_file_location("public_limits", project / "scripts/extract-board-reference.py")
boards_module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boards_module)
boards_module.POLICY_KEYS.add("MAX_COM_CHARS_AUTHED")
boards = {board["slug"]: board for board in boards_module.extract(source)["boards"]}
policies = []
for board in boards.values():
    value = board["source_policy"].get("MAX_COM_CHARS_AUTHED", "")
    if not re.fullmatch(r"[0-9]+", value) or not 1 <= int(value) <= 50000:
        raise ValueError("Authorized comment policy is outside audited bounds.")
    policies.append({"slug": board["slug"], "max_authorized_comment_chars": int(value)})
if len(policies) != 82:
    raise ValueError("Authorized policy inventory is incomplete.")

post = bodies["imgboard.php"]
begin = post.index('// Standardize new character lines', post.index('function new_post('))
end = post.index('$sub = normalize_content( $sub );', begin)
checks = post[begin:end]
if len(checks) > 2048 or checks.count('error(') != 6:
    raise ValueError("Input check block changed.")
line_checks = [line.strip() for line in post.splitlines()
               if 'if( !has_level() && substr_count( $com, "\\n" ) > MAX_LINES )' in line]
if len(line_checks) != 1:
    raise ValueError("Line check block changed.")
repeat_mark = post.index('$match = array();', post.index('function new_post('))
repeat_begin = post.rindex('if( !has_level() ) {', 0, repeat_mark)
repeat_end = post.index('// FIXME sanitize_text()', repeat_begin)
repeat_checks = post[repeat_begin:repeat_end]
if len(repeat_checks) > 2048 or repeat_checks.count('error(') != 1:
    raise ValueError("Repeated-line check block changed.")
auth = bodies["lib/auth.php"]
begin = auth.index("function has_level(")
end = auth.index("function has_flag(", begin)
level_body = auth[begin:end]
level_map = re.search(r'\$levelorderf\s*=\s*(array\([\s\S]*?\));', auth).group(1)
if len(level_body) > 2048 or len(level_map) > 512:
    raise ValueError("Rank check block exceeds audited bounds.")

output = {"reference": "operator-supplied 4chan-old checkout", "files": pinned_policy["files"] | hashes,
          "scope": "raw UTF-8 byte/scalar limits after newline normalization and isolated line/repetition admission",
          "boards": policies, "groups": []}
for slug in ["g", "j"]:
    board = boards[slug]
    public_limit = board["max_comment_chars"]
    authorized_limit = int(board["source_policy"]["MAX_COM_CHARS_AUTHED"])
    for role in ["janitor", "mod", "manager", "admin"]:
        recipes = []
        for field in ["name", "email", "sub"]:
            for char, repeat in [("n", 100), ("n", 101), ("n", 255), ("n", 256), ("界", 33), ("界", 34), ("界", 85), ("界", 86)]:
                recipes.append({"field": field, "char": char, "repeat": repeat})
        for limit in sorted({public_limit, authorized_limit}):
            for char in ["x", "😀"]:
                for repeat in [limit, limit + 1]:
                    recipes.append({"field": "com", "char": char, "repeat": repeat})
            for repeat in [limit // 2, limit // 2 + 1]:
                recipes.append({"field": "com", "char": "x\r\n", "repeat": repeat})
        recipes.append({"field": "com", "lines": board["comment_max_lines"] + 1})
        for repeat in [6, 8]:
            recipes.append({"field": "com", "char": "x\n", "repeat": repeat})
        program = ("if(!extension_loaded('mbstring')||mb_internal_encoding()!=='UTF-8'){throw new RuntimeException('UTF-8 mbstring is required.');} "
                   "define('YES',true); define('S_TOOLONG','too_long'); define('S_GENERICERROR','generic'); define('S_TOOMANYLINES','too_many_lines'); define('S_REJECTTEXTBAN','repeated_lines'); "
                   f"define('MAX_COM_CHARS',{public_limit}); define('MAX_COM_CHARS_AUTHED',{authorized_limit}); define('MAX_LINES',{board['comment_max_lines']}); "
                   f"$auth=['level'=>{json.dumps(role)}]; $levelorderf={level_map}; "
                   "function is_local_auth(){return false;} class OwnedLimitStop extends Exception{} function error($message,$dest=null){throw new OwnedLimitStop($message);} "
                   + level_body
                   + "function owned_check($recipe){$name='Owned';$email='';$sub='Owned';$com='Owned';$resto='0';$url='';$dest='synthetic';"
                   + "if(isset($recipe['lines'])){$value=implode(\"\\n\",array_map(fn($n)=>'line'.$n,range(0,$recipe['lines'])));}else{$value=str_repeat($recipe['char'],$recipe['repeat']);}"
                   + "${$recipe['field']}=$value;" + checks + repeat_checks + line_checks[0]
                   + "return ['accepted'=>true,'normalized_chars'=>mb_strlen($com),'raw_field_bytes'=>strlen($value),'normalized_newlines'=>substr_count($com,\"\\n\")];}"
                   + "$recipes=json_decode(stream_get_contents(STDIN),true,512,JSON_THROW_ON_ERROR);$results=[];foreach($recipes as $recipe){try{$results[]=owned_check($recipe);}catch(OwnedLimitStop $e){$results[]=['accepted'=>false,'error'=>$e->getMessage()];}}echo json_encode($results,JSON_THROW_ON_ERROR); ")
        run = subprocess.run(["php", "-d", "memory_limit=64M", "-d", "max_execution_time=5", "-r", program],
                             input=json.dumps(recipes), text=True, capture_output=True, timeout=10, check=True)
        if run.stderr:
            raise ValueError("Unexpected pure-source warning.")
        results = json.loads(run.stdout)
        output["groups"].append({"board": slug, "role": role, "public_chars": public_limit,
                                 "code": board["comment_code_spacing"], "sjis": board["comment_sjis_spacing"],
                                 "max_lines": board["comment_max_lines"], "spoilers": board["comment_spoiler_cleanup"],
                                 "authorized_chars": authorized_limit,
                                 "cases": [dict(recipe, **result) for recipe, result in zip(recipes, results, strict=True)]})
outputs = {args.output: json.dumps(output, ensure_ascii=False, indent=2) + "\n"}
if args.migration:
    rows = ["('" + policy["slug"] + "'," + str(policy["max_authorized_comment_chars"]) + ")" for policy in policies]
    outputs[args.migration] = ("-- Moderator-and-higher comment budgets from the pinned public configuration.\n"
        "ALTER TABLE content.boards ADD COLUMN max_authorized_comment_chars integer NOT NULL DEFAULT 10000\n"
        "    CHECK(max_authorized_comment_chars BETWEEN 1 AND 50000);\n"
        "GRANT SELECT(slug,max_comment_chars,max_authorized_comment_chars) ON content.boards TO board_staff_post_owner;\n"
        "CREATE POLICY staff_post_policy_metadata ON content.boards FOR SELECT TO board_staff_post_owner USING(true);\n"
        "UPDATE content.boards b SET max_authorized_comment_chars=policy.maximum FROM (VALUES\n"
        + ",\n".join(rows) + ") policy(slug,maximum) WHERE b.slug=policy.slug;\n")
for target, value in outputs.items():
    data = value.encode("utf-8")
    if args.check:
        if target.read_bytes() != data:
            raise ValueError("Authorized source reference or migration differs.")
    else:
        target.write_bytes(data)
print(json.dumps({"definitions": len(policies), "groups": len(output["groups"]),
                  "cases": sum(len(group["cases"]) for group in output["groups"])}))
