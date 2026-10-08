"""Execute the pinned remote staff cleanup permission helpers on synthetic accounts."""
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
    'lib/auth.php': '98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019',
}
sources = {}
for name, expected in hashes.items():
    raw = (args.source / name).read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('Audited source changed')
    sources[name] = raw.decode().replace('\r\n', '\n')
admin, auth = sources['admin.php'], sources['lib/auth.php']
predicate = "$title === 'Board Cleanup' && !has_level('manager') && !has_flag('developer')"
if predicate not in admin or "DELETE FROM r9k_posts WHERE created_on < DATE_SUB(NOW(), INTERVAL 2 YEAR)" not in admin:
    raise ValueError('Cleanup source boundary changed')
helpers = auth[auth.index('function has_level('):auth.index('function is_user()')]
levels = auth[auth.index('$levelorder ='):auth.index("if (!defined('SQLLOGMOD'))")]
# Each rank gets a fresh process because the real has_level helper caches rank.
cases = []
for role in ['janitor', 'mod', 'manager', 'admin']:
    program = levels + helpers + '''
function is_local_auth() { return false; }
$role=json_decode(stream_get_contents(STDIN),true);
$title='Board Cleanup'; $result=[];
foreach ([[],['r9k'],['all']] as $allow)
foreach ([[],['r9k'],['noboard'],['r9k','noboard']] as $deny)
foreach ([[],['developer']] as $flags) {
    $auth=['level'=>$role,'allow'=>$allow,'deny'=>$deny,'flags'=>$flags,'guest'=>false];
    $allowed=access_board('r9k') && has_level('mod') && !(PREDICATE);
    $result[]=compact('role','allow','deny','flags','allowed');
}
echo json_encode($result,JSON_THROW_ON_ERROR);
'''.replace('PREDICATE', predicate)
    run = subprocess.run([args.php, '-d', 'memory_limit=16M', '-r', program],
                         input=json.dumps(role).encode(), capture_output=True, timeout=5, check=True)
    if run.stderr:
        raise ValueError(run.stderr.decode())
    cases.extend(json.loads(run.stdout))
fixture = {'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
           'files': hashes, 'scope': 'Authenticated remote Board Cleanup permission checks only; no login, database, filesystem or local-auth execution',
           'cases': cases}
encoded = (json.dumps(fixture, indent=2)+'\n').encode()
if args.check:
    if args.output.read_bytes() != encoded:
        raise ValueError('Cleanup reference differs')
else:
    args.output.write_bytes(encoded)
print(json.dumps({'cases': len(cases)}))
