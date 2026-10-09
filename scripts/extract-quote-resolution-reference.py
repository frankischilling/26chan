"""Pinned, static PHP quote-resolution presentation reference.

python3 scripts/extract-quote-resolution-reference.py --source ../26chan-reference --write
python3 scripts/extract-quote-resolution-reference.py [--source ../26chan-reference]

No source application, SQL, network, or Rust is executed. Expectations below are
independently transcribed from captured PHP branches with controlled lookup answers.
Optional --php-oracle executes ONLY the hashed render functions and bounded stubs
in an isolated PHP process. Review before enabling this optional hosted-CI check.
"""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/quote-resolution-reference.json'
REVISION = '545b7812d1849f7958d914950c91fdbbe38f6b22'
EXCERPTS = {
    'allowlist': ('imgboard.php', 189, 189, 'affb920ce918619fecf27427caf849cff4f03cc93086e2f6e5af37d45e9d35df'),
    'same_lookup': ('imgboard.php', 3886, 3904, '8936dba49ec451a8b6a74c01746466e8d5b5e0aba7482595a5a4e918a14a8a3f'),
    'cross_lookup': ('imgboard.php', 3940, 3963, '2d0999e7e3bb5a627a2f0df7bcb332baad2d3f406acff093da94d14ef52758e2'),
    'boards_matching': ('imgboard.php', 4013, 4021, '38ba70c8771498a74326aaa8a65230f3e76b5bca43447a5856fdc03aad1074f9'),
    'auto_link': ('imgboard.php', 4103, 4139, '7ba7951e972f374441d9c5d3a8e3b4a82f6c2f4d427db8c126616aa57267eb61'),
    'same_render': ('imgboard.php', 4141, 4170, '520cb531dd3f833d2f5fa1293690be90e4f6149a1ed1f29dc807bc472c50e188'),
    'cross_render': ('imgboard.php', 4172, 4217, '9642244af77dea597e3952bab3cba9a5f2e2c84abb86d924e4e97f6211daa8c9'),
    'json_auto_link': ('json.php', 521, 521, '371cc213793d5d2bc7cca903eef8e555198da677621c96e2a283b40c6a4036e7'),
}
I64_MAX = 9223372036854775807


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical(value):
    return isinstance(value, str) and re.fullmatch(r'[1-9][0-9]*', value) is not None and int(value) <= I64_MAX


def decimal(value):
    return isinstance(value, str) and re.fullmatch(r'[0-9]+', value) is not None and int(value) <= I64_MAX


def capture(source):
    result = {}
    for name, (file, start, end, sha) in EXCERPTS.items():
        raw = b''.join((source / file).read_bytes().splitlines(keepends=True)[start-1:end])
        require(hashlib.sha256(raw).hexdigest() == sha, f'source drift: {name}')
        result[name] = raw.decode('utf-8')
    return result


def validate_captured(captured):
    require(set(captured) == set(EXCERPTS), 'captured excerpt names mismatch')
    for name, (_, _, _, sha) in EXCERPTS.items():
        require(hashlib.sha256(captured[name].encode()).hexdigest() == sha, f'captured source drift: {name}')


def make_cases(allowlist):
    cases = []

    def add(id, kind='same_board', board='g', source='g', current='100', no='123', resto='100', absence=None):
        require(decimal(no), f'nondecimal case: {id}')
        require(int(no) > 0 or resto is None, 'post zero must be absent')
        require(resto is None or resto == '0' or canonical(resto), 'invalid controlled resto')
        label = '&gt;&gt;' + no if kind == 'same_board' else '&gt;&gt;&gt;/' + board + '/' + no
        if kind == 'cross_board' and board not in allowlist:
            state, thread, href = 'plain', None, None
        elif resto is None or (kind == 'cross_board' and source == 'mlp' and board in ('b', 'co')):
            state, thread, href = 'dead', None, None
        else:
            thread = str(int(no)) if resto == '0' else resto
            if kind == 'same_board' and current and (int(current) == int(resto) or int(current) == int(no)):
                state, href = 'local', '#p' + no
            else:
                state = 'thread'
                prefix = '/' if kind == 'same_board' else '//boards.example.test/'
                href = prefix + board + '/thread/' + (no if resto == '0' else thread) + '#p' + no
        rendered = label if state == 'plain' else ('<span class="deadlink">' + label + '</span>' if state == 'dead' else '<a href="' + href + '" class="quotelink">' + label + '</a>')
        cases.append(dict(id=id, source_board=source, source_thread_id=current, token_kind=kind,
                          label_html=label, target_board=board, target_post_id=no, lookup_resto=resto,
                          absence_reason=absence, expected=dict(kind=state, thread_id=thread, href=href, html=rendered)))

    add('same-thread-op', no='100', resto='0')
    add('same-thread-reply')
    add('other-thread-op', resto='0')
    add('other-thread-reply', resto='200')
    add('board-index-op', current=None, resto='0')
    add('board-index-reply', current=None)
    for absence in ('missing', 'private-as-missing', 'deleted'):
        add('same-' + absence, resto=None, absence=absence)
        add('cross-' + absence, kind='cross_board', board='a', resto=None, absence=absence)
    add('cross-op', kind='cross_board', board='a', resto='0')
    add('cross-reply', kind='cross_board', board='a', resto='200')
    add('cross-same-board-remains-full-url', kind='cross_board')
    for board in ('unknown', 'test', 'global'):
        add('cross-allowlist-reject-' + board, kind='cross_board', board=board)
    for board in ('b', 'co'):
        for resto in ('0', '200', None):
            add('mlp-' + board + '-' + ('missing' if resto is None else resto), kind='cross_board', source='mlp', board=board, resto=resto)
    add('mlp-allowed-a', kind='cross_board', source='mlp', board='a')
    add('other-source-can-link-b', kind='cross_board', source='a', board='b')
    add('minimum-canonical-id', no='1', resto='0', current=None)
    add('maximum-canonical-op', no=str(I64_MAX), resto='0', current=None)
    add('maximum-canonical-reply', kind='cross_board', board='a', no=str(I64_MAX), resto=str(I64_MAX-1))
    add('numeric-board-allowlisted', kind='cross_board', board='3')
    # Every exact allowlisted board is exercised, including names absent from navigation lists.
    for board in allowlist:
        add('allowlisted-' + board, kind='cross_board', board=board, resto='0')
    for case in list(cases):
        add('leading-zero-' + case['id'], kind=case['token_kind'], board=case['target_board'],
            source=case['source_board'], current=case['source_thread_id'], no='000' + case['target_post_id'],
            resto=case['lookup_resto'], absence=case['absence_reason'])
    for no in ('0', '0000'):
        add('zero-same-' + no, no=no, resto=None, absence='post-zero-invariant')
        add('zero-cross-' + no, kind='cross_board', board='a', no=no, resto=None, absence='post-zero-invariant')
        add('zero-unknown-' + no, kind='cross_board', board='unknown', no=no, resto=None)
        add('zero-mlp-' + no, kind='cross_board', source='mlp', board='co', no=no, resto=None)
    return cases


def generate(captured):
    validate_captured(captured)
    match = re.fullmatch(r'\$valid_boards = "([a-z0-9|]+)";\s*', captured['allowlist'])
    require(match is not None, 'allowlist syntax changed')
    allowlist = match[1].split('|')
    cases = make_cases(allowlist)
    reply = next(c['expected']['html'] for c in cases if c['id'] == 'same-thread-reply')
    lexical = [
        dict(id='escaped-prose-preserved', source_board='g', source_thread_id='100', lookup_resto='100', input_html='&lt;b&gt; &amp; &quot; &gt;&gt;123 &lt;/b&gt;', expected_html='&lt;b&gt; &amp; &quot; ' + reply + ' &lt;/b&gt;'),
        dict(id='same-digit-prefix-before-letters', source_board='g', source_thread_id='100', lookup_resto='100', input_html='&gt;&gt;123abc', expected_html=reply + 'abc'),
        dict(id='same-digit-prefix-before-exponent', source_board='g', source_thread_id='100', lookup_resto='100', input_html='&gt;&gt;1e3', expected_html='<a href="#p1" class="quotelink">&gt;&gt;1</a>e3'),
        dict(id='double-escaped-not-reinterpreted', source_board='g', source_thread_id='100', lookup_resto='100', input_html='&amp;gt;&amp;gt;123', expected_html='&amp;gt;&amp;gt;123'),
        dict(id='uppercase-cross-board-is-plain', source_board='g', source_thread_id='100', lookup_resto='100', input_html='&gt;&gt;&gt;/G/123', expected_html='&gt;&gt;&gt;/G/123'),
    ]
    return dict(schema_version=1, source_revision=REVISION,
                oracle_method='static-extraction-and-independently-derived-controlled-lookup-expectations',
                source_execution='not performed while generating this fixture; optional --php-oracle validates extracted functions only',
                limits=['Decimal post spellings within signed i64, including leading zeroes, are qualified after source word wrapping.',
                        'Post zero is qualified only as controlled absence; the Rust store cannot contain post zero.',
                        'Signed, exponent, nondecimal and overflow IDs and general MySQL coercion are explicitly unqualified.',
                        'Same-board tokenizer consumes a digit prefix in >>123abc or >>1e3; that canonical prefix is qualified, not a claim about MySQL nondecimal coercion.',
                        'Lookup answers are controlled stubs. Private-as-missing is an absence presentation scenario, not evidence of source SQL RLS.',
                        'Source URL constants are controlled: NEW_HTML=true, RES_DIR2=thread/, PHP_EXT2=empty, L::d=example.test. Cross-host deployment mapping is not qualified.',
                        'Only already-escaped input is passed to auto_link; this is not an HTML sanitizer.'],
                excerpts={n: dict(file=v[0], start_line=v[1], end_line=v[2], sha256=v[3]) for n,v in EXCERPTS.items()},
                captured_sources=captured, allowlist=allowlist, cases=cases, lexical_cases=lexical,
                unqualified_ids=['+1', '-1', '1e3', '1.0', '0x10', str(I64_MAX+1)])


def load_fixture():
    fixture = json.loads(FIXTURE.read_text())
    require(fixture == generate(fixture['captured_sources']), 'fixture differs from pinned independent derivation')
    return fixture


def php_oracle(fixture):
    """Optional execution of only exact-hashed bounded functions, never source app."""
    php = shutil.which('php')
    require(php is not None, 'PHP unavailable; no PHP execution was performed')
    captured = fixture['captured_sources']
    validate_captured(captured)
    functions = '\n'.join(captured[n] for n in ('allowlist', 'same_lookup', 'cross_lookup', 'boards_matching', 'auto_link', 'same_render', 'cross_render'))
    harness = '''
if (PHP_INT_SIZE !== 8) { throw new RuntimeException('oracle requires 64-bit PHP'); }
const NEW_HTML = true;
const RES_DIR2 = 'thread/';
const PHP_EXT2 = '';
class L { static function d($board) { return 'example.test'; } }
function mysql_global_call($query) { global $c; return $c['allowlist']; }
function mysql_column_array($rows) { return $rows; }
function mysql_board_call($format, $board, $no) {
  global $resto, $c;
  if ($board !== $c['lookup_board'] || (string)(int)$no !== $c['lookup_post_id']
      || !in_array($format, ['SELECT resto FROM `%s` WHERE no=%d', 'select resto from `%s` where no=%d'], true)) {
    throw new RuntimeException('controlled lookup identity mismatch');
  }
  // Execute the exact source %d boundary without issuing a database request.
  if (sprintf('%d', $no) !== $c['lookup_post_id']) throw new RuntimeException('decimal conversion mismatch');
  return $resto;
}
function mysql_num_rows($rows) { return $rows === false ? 0 : 1; }
function mysql_fetch_row($rows) { return [$rows]; }
function mysql_result($rows, $index) { return $rows; }
$c = json_decode(stream_get_contents(STDIN), true);
define('BOARD_DIR', $c['source_board']);
define('SQLLOG', $c['source_board']);
$log = [];
$resto = $c['lookup_resto'] === null ? false : (int)$c['lookup_resto'];
echo auto_link($c['input_html'], $c['source_thread_id'] === null ? 0 : (int)$c['source_thread_id']);
'''
    count = 0
    for original in fixture['cases'] + fixture['lexical_cases']:
        case = dict(original)
        case['input_html'] = case.get('input_html', case.get('label_html'))
        case['allowlist'] = fixture['allowlist']
        digits = case.get('target_post_id')
        if digits is None:
            match = re.search(r'&gt;&gt;([0-9]+)', case['input_html'])
            digits = match[1] if match else '0'
        case['lookup_post_id'] = str(int(digits))
        case['lookup_board'] = case.get('target_board', case['source_board'])
        expected = case.get('expected_html', case.get('expected', {}).get('html'))
        result = subprocess.run([php, '-n', '-d', 'memory_limit=32M', '-d', 'max_execution_time=2', '-r', functions + harness], input=json.dumps(case), text=True, capture_output=True, timeout=5, check=True)
        require(result.stdout == expected and not result.stderr, f'PHP oracle mismatch: {case["id"]}: {result.stdout!r} {result.stderr!r}')
        count += 1
    print(f'isolated extracted PHP oracle: {count} cases passed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--php-oracle', action='store_true')
    args = parser.parse_args()
    require(not args.write or args.source is not None, '--write requires --source')
    if args.source is not None:
        fixture = generate(capture(args.source))
        if args.write:
            FIXTURE.write_text(json.dumps(fixture, indent=2, ensure_ascii=False) + '\n')
        else:
            require(fixture == load_fixture(), 'source and stored fixture differ')
    else:
        fixture = load_fixture()
    print(f'quote-resolution static reference: {len(fixture["cases"])} target cases, {len(fixture["lexical_cases"])} lexical cases; pinned excerpts verified; PHP not executed by static verification')
    if args.php_oracle:
        php_oracle(fixture)


if __name__ == '__main__':
    main()
