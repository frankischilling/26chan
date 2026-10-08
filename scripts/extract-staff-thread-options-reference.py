"""Execute bounded original perma-age/Undead preparation and audit masks."""
import argparse
import hashlib
import itertools
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
    'admin.php': '4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb',
    'lib/auth.php': '98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019',
}
bodies = {}
for name, expected in hashes.items():
    data = (args.source/name).read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('Audited source changed.')
    bodies[name] = data.decode('utf8').replace('\r\n', '\n')
admin, auth = bodies['admin.php'], bodies['lib/auth.php']
helpers = auth[auth.index('function has_level('):auth.index('function is_user()')]
level_map = re.search(r'\$levelorderf\s*=\s*(array\([\s\S]*?\));', auth).group(1)
option_start = admin.index('function adminopt()')
threshold = re.search(r"if \(\$title !== 'Ban Request' && !(has_level\('mod'\))\)", admin).group(1)
manager = re.search(r"\$is_managerplus = has_level\( 'manager' \) \|\| has_flag\('developer'\);", admin[option_start:]).group(0)
coercion = re.search(r'if\( !\$is_managerplus \) \$post_permaage = \$permaage;', admin[option_start:]).group(0)
write_start = admin.index('if( $post_permaage ) {', option_start)
writes = admin[write_start:admin.index('// Clear the undead flag', write_start)]
audit_start = admin.index('function log_thread_opts_action(')
audit_helper = admin[audit_start:admin.index('function get_board_options_html()',audit_start)]
if len(helpers)>4096 or len(level_map)>512 or len(manager)>128 or len(coercion)>128 or len(writes)>1024 or len(audit_helper)>4096:
    raise ValueError('Selected source exceeds audited bounds.')
rows, environment = [], None
for role in ['janitor', 'mod', 'manager', 'admin']:
    recipes = [dict(role=role, developer=developer, allow_all=allow_all, deny_noboard=deny,
                    old=old, desired=desired, old_undead=old_undead, undead=undead)
               for developer, allow_all, deny, old, desired, old_undead, undead
               in itertools.product(*([[False, True]]*7))]
    # Original has_level caches its rank, so roles use separate processes.
    # Legacy curly-string-offset syntax elsewhere in adminopt is not selected.
    program = (
        "function is_local_auth(){return false;}"
        "define('BOARD_DIR','g');$_COOKIE['4chan_auser']='Owned source fixture';"
        "function mysql_global_call($query,...$values){global $audit;$audit=['old_mask'=>$values[0],'new_mask'=>$values[1]];return true;}"
        f"$auth=['level'=>{json.dumps(role)},'guest'=>false,'allow'=>[],'deny'=>[],'flags'=>[]];$levelorderf={level_map};"
        + helpers + audit_helper
        + "$recipes=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);$rows=[];"
        "foreach($recipes as $recipe){$auth['allow']=$recipe['allow_all']?['all']:['g'];"
        "$auth['deny']=$recipe['deny_noboard']?['noboard']:[];"
        "$auth['flags']=$recipe['developer']?['developer']:[];"
        "$permaage=(int)$recipe['old'];$post_permaage=(int)$recipe['desired'];$post_undead=(int)$recipe['undead'];"
        + manager + coercion
        + "$vars='';ob_start();" + writes + "ob_end_clean();"
        "$audit=null;$post_data=['no'=>42,'sticky'=>0,'permasage'=>0,'closed'=>0,'permaage'=>$permaage,"
        "'undead'=>(int)$recipe['old_undead'],'name'=>'Anonymous','sub'=>'','com'=>'Owned comment','filename'=>'','ext'=>''];"
        "$logged=log_thread_opts_action($post_data,0,0,0,$post_permaage,$post_undead);"
        f"$rows[]=$recipe+['thread_options_allowed'=>{threshold},'permaage_allowed'=>$is_managerplus,"
        "'prepared_permaage'=>(bool)$post_permaage,'assignments'=>$vars,'logged'=>$logged,'audit'=>$audit];}"
        "echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
    )
    run = subprocess.run(['php','-d','memory_limit=64M','-d','max_execution_time=5','-r',program],
                         input=json.dumps(recipes).encode(),capture_output=True,timeout=10,check=True)
    if run.stderr or len(run.stdout)>131072:
        raise ValueError('Unexpected source option output.')
    result=json.loads(run.stdout)
    if len(result['cases'])!=len(recipes) or (environment is not None and result['php']!=environment):
        raise ValueError('Source audit omitted cases or changed runtime.')
    environment=result['php'];rows.extend(result['cases'])
fixture={
    'reference':'operator-supplied 4chan-old checkout',
    'source_revision':'545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files':hashes,'extractor_php':environment,
    'scope':'source thread-options rank threshold, perma-age manager/developer predicate and coercion, perma-age/Undead assignments, and changed/unchanged action masks; whole adminopt, sticky ordering, database writes and rebuilds excluded',
    'boundary_stubs':['synthetic rank, allow/deny scope and developer flag','disabled local authentication',
                      'synthetic saved/requested flags','discarded bounded source feedback','recorded synthetic audit call without database access'],
    'cases':rows,
}
data=(json.dumps(fixture,ensure_ascii=True,indent=2)+'\n').encode('utf8')
if len(rows)!=512 or len(data)>524288:
    raise ValueError('Thread-options fixture exceeds audited bounds.')
if args.check:
    if args.output.read_bytes()!=data:raise ValueError('Staff thread-options source reference differs.')
else:
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_bytes(data)
print(json.dumps({'cases':len(rows),'roles':sorted({row['role'] for row in rows})}))
