"""Run the pinned Options preparation and badge helpers with synthetic authority."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
hashes = {
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'lib/auth.php': '98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019',
}
bodies = {}
for relative, expected in hashes.items():
    data = (args.source / relative).read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('Audited source changed.')
    bodies[relative] = data.decode('utf8').replace('\r\n', '\n')
auth, post = bodies['lib/auth.php'], bodies['imgboard.php']
helpers = auth[auth.index('function has_level('):auth.index('function is_user()')]
level_map = re.search(r'\$levelorderf\s*=\s*(array\([\s\S]*?\));', auth).group(1)
capcode = post[post.index('function parse_capcode('):post.index('function generate_tim()')]
options_start = post.index('$is_sage = false;')
options = post[options_start:post.index('if( SPOILERS', options_start)]
name_start = post.index("if (!has_level('admin') && !has_flag('capcodename'))")
name_rule = post[name_start:post.index('// Only pass and authed users can use VIP capcodes', name_start)]
robot_start = post.index("if (defined('ROBOT9000') && ROBOT9000) {")
robot_block = post[robot_start:post.index("require_once 'plugins/robot9000.php';", robot_start)]
robot = "($options_field !== 'bypass_r9k' || !has_level('janitor')) && $capcode === 'none'"
if (len(helpers) > 4096 or len(level_map) > 512 or len(capcode) > 4096
        or len(options) > 512 or len(name_rule) > 256 or len(robot_block) > 512
        or robot_block.count(robot) != 1):
    raise ValueError('Selected source exceeds audited bounds.')
choices = ['', 'sage', 'SaGe', 'NONOKO', 'sageNONOKO', 'bypass_r9k',
           'bypass_r9ksage', 'bypass_r9k sage', 'capcode_mod', 'sagecapcode_mod',
           'capcode_dev', 'capcode_manager', 'capcode_admin', 'capcode_founder',
           'capcode_admin_hl', 'capcode_unknown']
rows, environment = [], None
for role in ['janitor', 'mod', 'manager', 'admin']:
    recipes = []
    for bits in range(8):
        flags = [flag for n, flag in enumerate(['capcode', 'developer', 'capcodename']) if bits & (1 << n)]
        for allow in [['all'], ['g']]:
            for deny in [[], ['noboard']]:
                recipes.extend({'role': role, 'flags': flags, 'allow_boards': allow,
                                'deny_boards': deny, 'input': choice} for choice in choices)
    # Source has_level caches its rank, so each role gets its own PHP process.
    # Null Pass/VIP names and disabled local auth keep those workflows excluded.
    program = (
        "define('YES',true);define('S_CANTCAPCODE','cant_capcode');define('S_ANONAME','Anonymous');"
        "function is_local_auth(){return false;}class OwnedOptionsStop extends Exception{}"
        "function error($message){throw new OwnedOptionsStop($message);}"
        "$_COOKIE=['4chan_auser'=>'owned-synthetic'];"
        f"$auth=['level'=>{json.dumps(role)},'guest'=>false,'allow'=>[],'deny'=>[],'flags'=>[]];$levelorderf={level_map};"
        + helpers + capcode
        + "$recipes=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);$rows=[];"
        "foreach($recipes as $recipe){$auth['flags']=$recipe['flags'];$auth['allow']=$recipe['allow_boards'];$auth['deny']=$recipe['deny_boards'];"
        "$email=$recipe['input'];$is_nonoko=false;$name='Owned finished name';$capcode='none';$outcome='none';"
        + options
        + "$options_field=$email;try{if(strpos($email,'capcode_')===0){"
        + name_rule + "$capcode=parse_capcode($email);}$outcome=$capcode;}"
        "catch(OwnedOptionsStop $e){$outcome=$e->getMessage();}"
        + "$robot_applies=" + robot + ";$authorized_limits=has_level();"
        "$rows[]=$recipe+['options_field'=>$options_field,'sage'=>$is_sage,'return_to_board'=>$is_nonoko,"
        "'outcome'=>$outcome,'name'=>$name,'robot_applies'=>$robot_applies,'authorized_limits'=>$authorized_limits];}"
        "echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
    )
    run = subprocess.run(['php', '-d', 'memory_limit=64M', '-d', 'max_execution_time=5', '-r', program],
                         input=json.dumps(recipes).encode(), capture_output=True, timeout=10, check=True)
    if run.stderr or len(run.stdout) > 262144:
        raise ValueError('Unexpected pure source output.')
    result = json.loads(run.stdout)
    if len(result['cases']) != len(recipes) or (environment is not None and result['php'] != environment):
        raise ValueError('Source audit omitted cases or changed runtime.')
    environment = result['php']
    rows.extend(result['cases'])
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': hashes, 'extractor_php': environment,
    'scope': 'authenticated Options preparation, conditional badge selection, name masking, authorized limits and Robot9000 applicability; posting and Pass/VIP excluded',
    'boundary_stubs': ['synthetic rank, flags and board scope', 'disabled local-auth override',
                       'synthetic username cookie', 'terminal error sentinel', 'null Pass/VIP name argument'],
    'cases': rows,
}
data = (json.dumps(fixture, ensure_ascii=True, indent=2)+'\n').encode('utf8')
if len(rows) != 2048 or len(data) > 1048576:
    raise ValueError('Options fixture exceeds audited bounds.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Staff posting Options reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({'cases': len(rows), 'outcomes': sorted({row['outcome'] for row in rows})}))
