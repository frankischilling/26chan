"""Extract complete target snapshots from two pinned legacy audit helpers only.

Example (official extracted PHP, no configuration/app initialization):
  LD_LIBRARY_PATH=/workspace/shared/26chan-tools/php/usr/lib/x86_64-linux-gnu \\
    python3 scripts/extract-staff-action-snapshots-reference.py 4chan-old \\
    apps/staff/tests/fixtures/staff-action-snapshots.json --check

The source accepts already-stored HTML strings. This oracle does not assert
byte-identical representation in the rewrite's structured post/audit model.
"""
import argparse
import hashlib
import json
import resource
import subprocess
import tempfile
from pathlib import Path

SOURCE_SHA256 = '4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb'
SELECTED_SHA256 = {
    'adminToggleSpoiler': '698b0c5fc2664227be9539993e49cae8a6b3179e2d4b4578a87b557994f430c0',
    'log_thread_opts_action': '08e17fbdcbab63049e2ac503d23c704c52f00099174cfab461fe4471ec9ccf3a',
}
NAMES = ('sticky', 'permasage', 'closed', 'permaage', 'undead')
MAX_OUTPUT = 524288
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--php', default='/workspace/shared/26chan-tools/php/usr/bin/php8.4')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
source_path = args.source / 'admin.php'
if source_path.stat().st_size > 2_000_000:
    raise ValueError('Source exceeds audited bound.')
raw = source_path.read_bytes()
if hashlib.sha256(raw).hexdigest() != SOURCE_SHA256:
    raise ValueError('Audited admin.php source changed.')
source = raw.decode('utf8').replace('\r\n', '\n')
selected = {}
for name, following in [('adminToggleSpoiler', 'adminopt'),
                        ('log_thread_opts_action', 'get_board_options_html')]:
    code = source[source.index('function ' + name + '('):
                  source.index('function ' + following + '(')]
    if len(code) > 2048 or hashlib.sha256(code.encode()).hexdigest() != SELECTED_SHA256[name]:
        raise ValueError('Audited selected helper changed: ' + name)
    selected[name] = code

# Synthetic stored-row values, deliberately never interpreted as instructions.
# Each profile carries filename punctuation plus an independently stored extension.
profiles = [
    dict(profile='named', name='Alice Example', sub='Named subject',
         com='First line<br>Second line', filename='holiday.photo', ext='.jpg',
         actor='fixture-moderator'),
    dict(profile='trip_like', name='Alice <span class="postertrip">!AbC123</span>',
         sub='Trip-like stored name', com='<span class="quote">&gt;quoted</span>',
         filename='archive.tar', ext='.png', actor='Fixture Staff !ActorTrip'),
    dict(profile='hostile_markup', name='&lt;script&gt;alert(&quot;name&quot;)&lt;/script&gt;',
         sub='&lt;img src=x onerror=alert(1)&gt; &amp; "subject"',
         com='<a href="javascript:alert(1)">synthetic hostile markup</a>\n\'quoted\'',
         filename='&lt;image&gt;.backup', ext='.webm', actor='fixture<&"\'actor>'),
    dict(profile='unicode', name='雪だるま 🧪', sub='Résumé · 東京 · مرحبا',
         com='你好<br>café e\u0301 🧵', filename='画像.版本', ext='.png', actor='担当者-é'),
]
recipes = []
for profile in profiles:
    for target in ('op', 'reply'):
        post = {key: profile[key] for key in ('name', 'sub', 'com', 'filename', 'ext')}
        post.update(no=420 if target == 'op' else 421, resto=0 if target == 'op' else 420)
        post.update({name: int(bool(21 & (1 << bit))) for bit, name in enumerate(NAMES)})
        common = dict(profile=profile['profile'], target=target, actor=profile['actor'], board='g')
        # All five action-mask bits change in 21 -> 10; 21 -> 21 is a no-op.
        for new_mask in (10, 21):
            recipes.append(common | dict(id=f"{profile['profile']}_{target}_options_{new_mask}",
                operation='thread_options', post=post.copy(), old_mask=21, requested_mask=new_mask))
        for old_spoiler, desired in ((False, True), (True, False), (False, False), (True, True)):
            row = post | {'sub': ('SPOILER<>' if old_spoiler else '') + post['sub']}
            recipes.append(common | dict(id=f"{profile['profile']}_{target}_spoiler_{int(old_spoiler)}_{int(desired)}",
                operation='spoiler', post=row, old_spoiler=old_spoiler, requested_spoiler=desired))

program = r'''
define('BOARD_DIR', 'g');
function record_call(&$calls, $args, $expected) {
    if (count($calls) >= 1 || count($args) !== $expected) {
        throw new Exception('Unexpected SQL call count or argument count');
    }
    foreach ($args as $arg) {
        if ((!is_string($arg) && !is_int($arg)) || (is_string($arg) && strlen($arg) > 2048)) {
            throw new Exception('SQL recorder exceeded synthetic bounds');
        }
    }
    $calls[] = ['query' => $args[0], 'arguments' => array_slice($args, 1)];
    return true;
}
function mysql_board_call(...$args) {
    global $board_calls;
    return record_call($board_calls, $args, 3);
}
function mysql_global_call(...$args) {
    global $global_calls, $recipe;
    return record_call($global_calls, $args, $recipe['operation'] === 'spoiler' ? 9 : 10);
}
''' + ''.join(selected.values()) + r'''
$recipes = json_decode(stream_get_contents(STDIN, 131073), true, 32, JSON_THROW_ON_ERROR);
if (count($recipes) !== 48) throw new Exception('Unexpected recipe count');
$rows = [];
foreach ($recipes as $recipe) {
    $_COOKIE = ['4chan_auser' => $recipe['actor']];
    $board_calls = []; $global_calls = [];
    $post = $recipe['post'];
    if ($recipe['operation'] === 'spoiler') {
        $returned = adminToggleSpoiler($post, $recipe['requested_spoiler']);
    } else {
        $mask = $recipe['requested_mask'];
        $returned = log_thread_opts_action($post, $mask & 1, $mask & 2,
            $mask & 4, $mask & 8, $mask & 16);
    }
    $rows[] = $recipe + ['source' => ['returned' => $returned,
        'mysql_board_calls' => $board_calls, 'mysql_global_calls' => $global_calls]];
}
echo json_encode(['php' => PHP_VERSION, 'cases' => $rows], JSON_THROW_ON_ERROR);
'''
payload = json.dumps(recipes, ensure_ascii=True).encode()
if len(program) > 8192 or len(payload) > 131072:
    raise ValueError('Synthetic harness exceeds audited bounds.')


def bound_child():
    resource.setrlimit(resource.RLIMIT_CPU, (5, 5))
    resource.setrlimit(resource.RLIMIT_FSIZE, (MAX_OUTPUT, MAX_OUTPUT))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


# One bounded process; disk-backed stdout/stderr avoid unbounded capture buffers.
with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
    result = subprocess.run([args.php, '-n', '-d', 'memory_limit=32M', '-d',
        'max_execution_time=5', '-r', program], input=payload, stdout=stdout, stderr=stderr,
        timeout=10, check=True, preexec_fn=bound_child)
    stdout.seek(0); output = stdout.read(MAX_OUTPUT + 1)
    stderr.seek(0); errors = stderr.read(4097)
if errors or len(output) > MAX_OUTPUT:
    raise ValueError('Unexpected source helper output: ' + errors[:500].decode('utf8', 'replace'))
result = json.loads(output)
rows = result['cases']
if len(rows) != 48 or [row['id'] for row in rows] != [row['id'] for row in recipes]:
    raise ValueError('Source helper omitted or reordered cases.')
for row in rows:
    actual = row['source']
    spoiler = row['operation'] == 'spoiler'
    changed = (row['old_spoiler'] != row['requested_spoiler']) if spoiler else (row['old_mask'] != row['requested_mask'])
    if actual['returned'] is not changed or len(actual['mysql_global_calls']) != int(changed):
        raise ValueError('Unexpected source audit/no-op behavior.')
    if len(actual['mysql_board_calls']) != int(spoiler and changed):
        raise ValueError('Unexpected source mutation/no-op behavior.')
    actual['audit_snapshot'] = None
    if not changed:
        continue
    call = actual['mysql_global_calls'][0]
    # Spoiler oldmask is a literal 0 in SQL, not a positional argument.
    values = ([0] + call['arguments']) if spoiler else call['arguments']
    snapshot = dict(zip(('oldmask', 'newmask', 'postno', 'board', 'name', 'sub', 'com', 'filename', 'admin'), values, strict=True))
    post = row['post']
    expected = dict(oldmask=0 if spoiler else row['old_mask'],
        newmask=(129 if row['requested_spoiler'] else 130) if spoiler else row['requested_mask'],
        postno=post['no'], board=row['board'], name=post['name'], sub=post['sub'],
        com=post['com'], filename=post['filename'] + post['ext'], admin=row['actor'])
    if snapshot != expected:
        raise ValueError('Source target snapshot changed.')
    actual['audit_snapshot'] = snapshot
    if spoiler:
        old_subject = post['sub'][9:] if row['old_spoiler'] else post['sub']
        new_subject = ('SPOILER<>' if row['requested_spoiler'] else '') + old_subject
        if actual['mysql_board_calls'][0]['arguments'] != [new_subject, post['no']]:
            raise ValueError('Source spoiler mutation differs from pre-mutation audit subject.')
fixture = dict(
    reference='operator-supplied 4chan-old checkout',
    source_revision='545b7812d1849f7958d914950c91fdbbe38f6b22',
    files={'admin.php': SOURCE_SHA256}, selected_snippet_sha256=SELECTED_SHA256,
    extractor_php=result['php'],
    scope='Complete SQL query and every positional mysql_global_call argument from adminToggleSpoiler and log_thread_opts_action; OP/reply saved-row target snapshots, changed masks, spoiler actions 129/130, pre-mutation spoiler subject, and no-op absence.',
    representation_boundary='Source name/sub/com are already-stored HTML strings and are forwarded unchanged. filename is filename + ext. These source bytes are not a claim of byte-identical representation in the structured rewrite.',
    excluded=['application initialization', 'real SQL/database/network', 'authentication and permissions',
              'whole grouped-options handler and persistence', 'rebuilds', 'rewrite representation equivalence'],
    boundary_stubs=['synthetic saved post rows and actor cookie', 'constant board g',
                    'successful bounded board/global SQL recorders; no connection or query execution'],
    snapshot_mapping='audit_snapshot names the captured SQL values; spoiler oldmask 0 comes from the literal in the captured query, all other fields from captured positional arguments.',
    cases=rows)
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
if len(data) > MAX_OUTPUT:
    raise ValueError('Snapshot fixture exceeds audited bound.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Staff action snapshot source reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps(dict(cases=len(rows), audits=sum(bool(row['source']['mysql_global_calls']) for row in rows),
    no_ops=sum(not row['source']['returned'] for row in rows), bytes=len(data))))
