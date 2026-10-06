"""Execute only the pinned thread-option audit helper on synthetic flag inputs."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
hashes = {
    'admin.php': '4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb',
    'reports/js/d8d9b0cdc33f3418/reportqueue-mod.js': 'bcac85ef9d14eb9c2a9ace5e4c27718f92080f760939867d7c46293e0935435f',
}
sources = {}
for name, expected in hashes.items():
    raw = (args.source / name).read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('Audited source changed.')
    sources[name] = raw.decode('utf8').replace('\r\n', '\n')
admin = sources['admin.php']
start = admin.index('function log_thread_opts_action(')
helper = admin[start:admin.index('function get_board_options_html()', start)]
helper_hash = hashlib.sha256(helper.encode()).hexdigest()
if len(helper) > 2048 or helper_hash != '08e17fbdcbab63049e2ac503d23c704c52f00099174cfab461fe4471ec9ccf3a':
    raise ValueError('Unexpected audit helper boundary.')

names = ('sticky', 'permasage', 'closed', 'permaage', 'undead')


def flags(mask):
    return {name: bool(mask & (1 << bit)) for bit, name in enumerate(names)}


recipes = []
for old_mask in range(32):
    for action, bit, requested in [('close', 4, True), ('reopen', 4, False),
                                   ('permasage', 2, True), ('unpermasage', 2, False)]:
        new_mask = old_mask | bit if requested else old_mask & ~bit
        recipes.append({'action': action, 'old': flags(old_mask), 'prepared': flags(new_mask)})
# These are explicitly supplied prepared states, not executed request parsing or
# rank authorization. The sparse RQ request at reportqueue-mod.js:792-811 sends
# only submit/permasage/token; adminopt:3521-3526 zeroes missing flags and :3549
# preserves permaage only when its permission is absent. No JS or adminopt runs.
sparse = [
    {'label': 'already_permasage_no_siblings', 'old': flags(2), 'prepared': flags(2)},
    {'label': 'already_permasage_clears_sticky_closed_undead', 'old': flags(23), 'prepared': flags(2)},
    {'label': 'already_permasage_preserves_protected_permaage', 'old': flags(31), 'prepared': flags(10)},
    {'label': 'already_permasage_permitted_permaage_reset', 'old': flags(31), 'prepared': flags(2)},
]
program = (
    "define('BOARD_DIR','g');$_COOKIE['4chan_auser']='Owned source fixture';"
    "function mysql_global_call($query,...$values){global $record;"
    "$record=['old_mask'=>$values[0],'new_mask'=>$values[1]];return true;}"
    + helper
    + "$recipes=json_decode(stream_get_contents(STDIN),true,16,JSON_THROW_ON_ERROR);$rows=[];"
    "foreach($recipes as $recipe){$post=$recipe['old']+['no'=>42,'name'=>'Anonymous','sub'=>'',"
    "'com'=>'Owned comment','filename'=>'','ext'=>''];$p=$recipe['prepared'];$record=null;"
    "$logged=log_thread_opts_action($post,$p['sticky'],$p['permasage'],$p['closed'],$p['permaage'],$p['undead']);"
    "$rows[]=$recipe+['logged'=>$logged,'audit'=>$record];}"
    "echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
)
if len(program) > 4096:
    raise ValueError('Harness exceeds audited bounds.')
run = subprocess.run(
    [args.php, '-n', '-d', 'memory_limit=32M', '-d', 'max_execution_time=5', '-r', program],
    input=json.dumps(recipes + sparse).encode(), capture_output=True, timeout=10, check=True,
)
if run.stderr or len(run.stdout) > 65536:
    raise ValueError('Unexpected isolated audit output.')
result = json.loads(run.stdout)
if len(result['cases']) != 132:
    raise ValueError('Incomplete audit cases.')
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': hashes,
    'selected_sha256': {'audit_helper': helper_hash},
    'extractor_php': result['php'],
    'scope': 'only log_thread_opts_action; 32 old masks times four isolated actions with unchanged siblings; no endpoint, assignments, rank checks, archive checks, timestamp evaluation, database or rebuild execution',
    'boundary_stubs': ['synthetic old and prepared flags', 'fixed synthetic post metadata and actor',
                       'audit SQL recorder without database access'],
    'cases': result['cases'][:128],
    'sparse_reportqueue_scope': 'illustrative explicitly prepared sparse-request outcomes from reportqueue-mod.js:792-811 and adminopt:3521-3526,3549; NOT current isolated controls and NOT execution/qualification of request parsing or permissions',
    'sparse_reportqueue_examples': result['cases'][128:],
}
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
if len(data) > 131072:
    raise ValueError('Fixture exceeds audited bounds.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Staff isolated option reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({'isolated_cases': 128, 'sparse_examples': 4, 'php': result['php'],
                  'audit_helper_sha256': helper_hash}))
