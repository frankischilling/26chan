"""Run only the pinned forcearchive function against synthetic boundary stubs.

No source includes, original application, database, network or archive_thread body
are run. Regenerate with the supplied PHP runtime and --check to verify the fixture.
"""
import argparse
import hashlib
import json
import resource
import subprocess
import tempfile
from pathlib import Path

SOURCE_SHA256 = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
SELECTED_SHA256 = 'd5952492aae3003b893d116297f8d1d125a5d051c9bc3d7ecaf4ce2bde3226f2'
MAX_OUTPUT = 65536
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--php', default='/workspace/shared/26chan-tools/php/usr/bin/php8.4')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
path = args.source / 'imgboard.php'
if path.stat().st_size > 1_000_000:
    raise ValueError('Source exceeds audited bound.')
raw = path.read_bytes()
if hashlib.sha256(raw).hexdigest() != SOURCE_SHA256:
    raise ValueError('Audited imgboard.php source changed.')
source = raw.decode('utf8').replace('\r\n', '\n')
start = source.index('function forcearchive() {')
selected = source[start:source.index('// Called remotely by other tools', start)]
if len(selected) > 2048 or hashlib.sha256(selected.encode()).hexdigest() != SELECTED_SHA256:
    raise ValueError('Audited forcearchive excerpt changed.')

post = dict(no=420, resto=0, sticky=0, archived=0, undead=0, name='Fixture moderator',
            sub='SPOILER<>Synthetic subject', com='Saved &lt;tag&gt;<br>雪',
            filename='image.version', ext='.png')
base = dict(moderator=True, archive_enabled=True, id_present=True, requested_id='420',
            query_ok=True, json_enabled=True, thread=post)
recipes = []

def case(name, error=None, **changes):
    recipes.append(base | changes | dict(id=name, expected_error=error))

case('rank_before_disabled_and_missing', "Can't let you do that.", moderator=False,
     archive_enabled=False, id_present=False)
case('disabled_before_missing', 'Archives are disabled on this board.',
     archive_enabled=False, id_present=False)
case('missing_id', 'Bad Request.', id_present=False)
case('query_failure', 'Database error.', query_ok=False)
case('missing_thread', 'Thread not found.', thread=None)
case('reply_before_archived_and_sticky', 'Thread not found.',
     thread=post | dict(no=421, resto=420, archived=1, sticky=1))
case('archived_before_sticky', 'This thread is already archived.',
     thread=post | dict(archived=1, sticky=1))
case('sticky', 'fixture-sticky-denial', thread=post | dict(sticky=1))
case('ordinary')
case('undead_allowed', thread=post | dict(undead=1))
case('json_disabled', json_enabled=False)
case('php_integer_coercion', requested_id='420trailing')
case('empty_saved_content', thread=post | dict(name='', sub='', com='', filename='', ext=''))

# This is the complete audited executable harness. Every external call made by
# the selected helper is implemented here as a bounded synthetic recorder.
program = r'''
$r = json_decode(stream_get_contents(STDIN, 8193), true, 16, JSON_THROW_ON_ERROR);
$calls = []; $audit = null;
define('BOARD_DIR', 'g');
define('ENABLE_ARCHIVE', $r['archive_enabled']);
define('ENABLE_JSON_THREADS', $r['json_enabled']);
define('S_MAYNOTDELSTICKY', 'fixture-sticky-denial');
function record_call($name, $args = []) {
    global $calls;
    if (count($calls) >= 8) throw new Exception('Recorder overflow');
    $calls[] = ['name' => $name, 'arguments' => $args];
}
function has_level() { global $r; return $r['moderator']; }
function error($text) { throw new DomainException($text); }
function mysql_board_call(...$args) {
    global $r;
    if (count($args) !== 3 || !is_int($args[2])) throw new Exception('Unexpected query');
    record_call('mysql_board_call', $args);
    return $r['query_ok'];
}
function mysql_fetch_assoc($result) { global $r; return $r['thread']; }
function archive_thread($id) { record_call('archive_thread', [$id]); }
function log_mod_action($action, $post) {
    global $audit;
    $audit = ['action' => $action, 'post' => $post];
    record_call('log_mod_action', [$action, $post]);
}
function generate_board_archived_json() { record_call('generate_board_archived_json'); }
function updating_index() { record_call('updating_index'); }
''' + selected + r'''
$_POST = $r['id_present'] ? ['id' => $r['requested_id']] : [];
$error = null;
try { forcearchive(); } catch (DomainException $e) { $error = $e->getMessage(); }
echo json_encode(['php' => PHP_VERSION, 'error' => $error,
    'calls' => $calls, 'audit' => $audit], JSON_THROW_ON_ERROR);
'''
if len(program) > 8192:
    raise ValueError('Harness exceeds audited bound.')

def bound_child():
    resource.setrlimit(resource.RLIMIT_CPU, (3, 3))
    resource.setrlimit(resource.RLIMIT_FSIZE, (MAX_OUTPUT, MAX_OUTPUT))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))

rows = []
php_version = None
for recipe in recipes:
    payload = json.dumps(recipe, ensure_ascii=True).encode()
    if len(payload) > 8192:
        raise ValueError('Recipe exceeds audited bound.')
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        subprocess.run([args.php, '-n', '-d', 'memory_limit=16M', '-d',
                        'max_execution_time=3', '-r', program], input=payload,
                       stdout=stdout, stderr=stderr, timeout=5, check=True,
                       preexec_fn=bound_child)
        stdout.seek(0)
        output = stdout.read(MAX_OUTPUT + 1)
        stderr.seek(0)
        errors = stderr.read(4097)
    if errors or len(output) > MAX_OUTPUT:
        raise ValueError('Unexpected helper output: ' + errors[:500].decode('utf8', 'replace'))
    actual = json.loads(output)
    version = actual.pop('php')
    if php_version is not None and version != php_version:
        raise ValueError('PHP runtime changed during extraction.')
    php_version = version
    if actual['error'] != recipe['expected_error']:
        raise ValueError('Source rejection changed: ' + recipe['id'])
    names = [call['name'] for call in actual['calls']]
    if recipe['expected_error'] is None:
        expected = ['mysql_board_call', 'archive_thread', 'log_mod_action']
        if recipe['json_enabled']:
            expected.append('generate_board_archived_json')
        expected.append('updating_index')
        snapshot = {key: recipe['thread'][key] for key in ('no', 'name', 'sub', 'com', 'filename', 'ext')}
        if names != expected or actual['audit'] != dict(action=3, post=snapshot):
            raise ValueError('Source call order or saved OP snapshot changed.')
    elif actual['audit'] is not None or any(name != 'mysql_board_call' for name in names):
        raise ValueError('Rejected source action reached a mutation boundary.')
    rows.append(recipe | dict(source=actual))
fixture = dict(
    reference='operator-supplied 4chan-old checkout',
    source_revision='545b7812d1849f7958d914950c91fdbbe38f6b22',
    files={'imgboard.php': SOURCE_SHA256},
    selected_snippet_sha256={'forcearchive': SELECTED_SHA256},
    extractor_php=php_version,
    scope='Executed forcearchive rejection order, boundary-call order and action-3 saved OP payload using synthetic rows.',
    excluded=['original application and includes', 'actual authentication and board permissions',
              'real SQL/database/network', 'archive_thread implementation and its mutations',
              'log_mod_action implementation and persistence', 'actual rendering or rebuilds',
              'rewrite HTTP error/status equivalence'],
    boundary_stubs=['has_level supplied as a boolean; default moderator rank verified separately in lib/auth.php',
                    'ENABLE_ARCHIVE and ENABLE_JSON_THREADS supplied per synthetic case',
                    'SQL returns one supplied row or failure; no actual query execution',
                    'archive, log and rebuild calls are recorders only',
                    'sticky denial constant is a synthetic sentinel'],
    representation_boundary='Stored source name/sub/com are forwarded unchanged. filename and ext remain separate at log_mod_action entry. This does not assert byte-identical rewrite structured snapshots. PHP integer coercion is recorded, not prescribed for rewrite request validation.',
    cases=rows)
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
if len(data) > MAX_OUTPUT:
    raise ValueError('Fixture exceeds audited bound.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Force archive reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps(dict(cases=len(rows), successful=sum(row['source']['error'] is None for row in rows),
                      bytes=len(data))))
