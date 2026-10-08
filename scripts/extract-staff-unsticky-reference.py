"""Execute only pinned unsticky assignments and unchanged-other-flag audit masks."""
import argparse
import hashlib
import itertools
import json
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
expected = '4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb'
raw = (args.source / 'admin.php').read_bytes()
if hashlib.sha256(raw).hexdigest() != expected:
    raise ValueError('Audited admin.php changed.')
source = raw.decode('utf8').replace('\r\n', '\n')
start = source.index('function adminopt()')
branch_start = source.index('if( $sticky == 1 ) {', start)
branch_end = source.index('\n\t\t}\n\t\tif( $post_permasage == 1 )', branch_start)
branch = source[branch_start:branch_end]
audit_start = source.index('function log_thread_opts_action(')
audit_end = source.index('function get_board_options_html()', audit_start)
audit = source[audit_start:audit_end]
# Reviewed selection: only local assignments, bounded echo and the audit helper.
# Neither adminopt itself nor its SQL, authentication, renderer, timestamp/rank
# encoding, commented Undead clearing or rebuild paths is executed.
if len(branch) > 512 or len(audit) > 2048:
    raise ValueError('Selected source exceeds audited bounds.')
if branch.count('$vars .=') != 1 or 'sticky=0,root=' not in branch:
    raise ValueError('Unexpected unsticky assignment boundary.')
recipes = [dict(sticky=sticky, permasage=sage, closed=closed,
                permaage=age, undead=undead)
           for sticky, sage, closed, age, undead
           in itertools.product([False, True], repeat=5)]
program = (
    "define('BOARD_DIR','g');$_COOKIE['4chan_auser']='Owned source fixture';"
    "function mysql_global_call($query,...$values){global $record;"
    "$record=['old_mask'=>$values[0],'new_mask'=>$values[1]];return true;}"
    + audit
    + "$recipes=json_decode(stream_get_contents(STDIN),true,16,JSON_THROW_ON_ERROR);$rows=[];"
    "foreach($recipes as $recipe){$sticky=(int)$recipe['sticky'];$vars='';"
    "ob_start();" + branch + "$feedback=ob_get_clean();"
    "if(strlen($feedback)>128){throw new Exception('Unexpected feedback.');}"
    "$record=null;$post=['no'=>42,'sticky'=>$sticky,'permasage'=>(int)$recipe['permasage'],"
    "'closed'=>(int)$recipe['closed'],'permaage'=>(int)$recipe['permaage'],"
    "'undead'=>(int)$recipe['undead'],'name'=>'Anonymous','sub'=>'','com'=>'Owned comment','filename'=>'','ext'=>''];"
    "$logged=log_thread_opts_action($post,0,$post['permasage'],$post['closed'],$post['permaage'],$post['undead']);"
    "$rows[]=$recipe+['requested_sticky'=>false,'assignments'=>$vars,'root_expression'=>$sticktime,'logged'=>$logged,'audit'=>$record];}"
    "echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
)
if len(program) > 8192:
    raise ValueError('PHP harness exceeds audited bounds.')
run = subprocess.run(
    [args.php, '-n', '-d', 'memory_limit=32M', '-d', 'max_execution_time=5', '-r', program],
    input=json.dumps(recipes).encode(), capture_output=True, timeout=10, check=True,
)
if run.stderr or len(run.stdout) > 32768:
    raise ValueError('Unexpected source unsticky output.')
result = json.loads(run.stdout)
rows = result['cases']
if len(rows) != 32:
    raise ValueError('Source unsticky cases are incomplete.')
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': {'admin.php': expected},
    'selected_sha256': {
        'unsticky_branch': hashlib.sha256(branch.encode()).hexdigest(),
        'audit_helper': hashlib.sha256(audit.encode()).hexdigest(),
    },
    'extractor_php': result['php'],
    'scope': 'requested sticky=false only; original unsticky root expression and audit masks with all other flags unchanged; permissions, whole endpoint, sticky ranks, database time evaluation, SQL execution and rebuilds excluded',
    'boundary_stubs': ['synthetic saved flags and fixed requested sticky=false',
                       'bounded discarded source feedback',
                       'synthetic post metadata and audit SQL recorder without database access'],
    'cases': rows,
}
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
if len(data) > 65536:
    raise ValueError('Fixture exceeds audited bounds.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Staff unsticky reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({'cases': len(rows), 'php': result['php'],
                  'selected_sha256': fixture['selected_sha256']}))
