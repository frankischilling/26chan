"""Run only the pinned Robot9000 and authentication bypass predicates."""
import argparse
import hashlib
import json
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
post = bodies['imgboard.php']
start = post.index("if (defined('ROBOT9000') && ROBOT9000) {")
robot_block = post[start:post.index("require_once 'plugins/robot9000.php';", start)]
robot_predicate = "($options_field !== 'bypass_r9k' || !has_level('janitor')) && $capcode === 'none'"
auth = bodies['lib/auth.php']
start = auth.index('function valid_captcha_bypass()')
auth_block = auth[start:auth.index('if (CAPTCHA != 1)', start)]
auth_predicate = "is_local_auth() || has_level('janitor')"
if robot_block.count(robot_predicate) != 1 or len(robot_block.encode()) > 512:
    raise ValueError('Robot9000 predicate changed.')
if auth_block.count(auth_predicate) != 1 or len(auth_block.encode()) > 512:
    raise ValueError('Authentication bypass predicate changed.')
payload = {
    'options': ['', 'bypass_r9k', 'BYPASS_R9K', 'bypass_r9k ', ' bypass_r9k', 'bypass_r9k sage', 'capcode_mod', 'sage'],
    'capcodes': ['none', 'mod', 'admin', 'admin_highlight', 'manager', 'developer', 'founder'],
}
program = '''
function has_level($level) {
    if ($level !== 'janitor') throw new Exception('Unexpected helper call');
    return $GLOBALS['janitor_or_higher'];
}
function is_local_auth() { return $GLOBALS['local_auth']; }
$inputs=json_decode(stream_get_contents(STDIN), true, flags:JSON_THROW_ON_ERROR);
$robot_cases=[];
foreach ([false,true] as $enabled)
foreach ([false,true] as $janitor_or_higher)
foreach ($inputs['options'] as $options_field)
foreach ($inputs['capcodes'] as $capcode) {
    $applies=$enabled && (ROBOT_PREDICATE);
    $robot_cases[]=compact('enabled','janitor_or_higher','options_field','capcode','applies');
}
$bypass_cases=[];
foreach ([false,true] as $local_auth)
foreach ([false,true] as $janitor_or_higher) {
    $bypass=AUTH_PREDICATE;
    $bypass_cases[]=compact('local_auth','janitor_or_higher','bypass');
}
echo json_encode(['php'=>PHP_VERSION,'robot_cases'=>$robot_cases,'bypass_cases'=>$bypass_cases],flags:JSON_THROW_ON_ERROR);
'''.replace('ROBOT_PREDICATE', robot_predicate).replace('AUTH_PREDICATE', auth_predicate)
run = subprocess.run(['php', '-d', 'memory_limit=16M', '-d', 'max_execution_time=5', '-r', program],
                     input=json.dumps(payload).encode(), capture_output=True, check=True, timeout=5)
if run.stderr or len(run.stdout) > 65536:
    raise ValueError('Unexpected pure predicate output.')
fixture = json.loads(run.stdout)
if len(fixture['robot_cases']) != 224 or len(fixture['bypass_cases']) != 4:
    raise ValueError('Predicate audit omitted cases.')
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': hashes,
    'scope': 'Robot9000 applicability and the early authenticated CAPTCHA/range-ban bypass; other admission rules excluded',
    'boundary_stubs': ['synthetic janitor-or-higher result', 'synthetic local-auth result', 'configured Robot9000 switch'],
    **fixture,
}
encoded = (json.dumps(fixture, indent=2, ensure_ascii=True)+'\n').encode('utf8')
if len(encoded) > 65536:
    raise ValueError('Predicate fixture exceeds audited bounds.')
if args.check:
    if args.output.read_bytes() != encoded:
        raise ValueError('Staff posting authority reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(encoded)
print(json.dumps({'robot_cases': 224, 'bypass_cases': 4}))
