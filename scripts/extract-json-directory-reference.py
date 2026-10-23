#!/usr/bin/env python3
"""Execute the pinned catalog-info generator with public INI fields only."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PIN = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
INI_PIN = '05a89bb56d627f9f5d0a07f3200a3efc9fbe50e6d84eb7e68e86d012fb318567'
KEYS = '''CATEGORY DEF_PAGES PAGE_MAX MAX_KB MAX_WEBM_FILESIZE MAX_COM_CHARS
MAX_WEBM_DURATION MAX_RES MAX_IMGRES RENZOKU3 RENZOKU RENZOKU2 META_DESCRIPTION
SPOILERS SPOILER_NUM DISP_ID ENABLE_ARCHIVE CODE_TAGS SJIS_TAGS JSMATH
SHOW_COUNTRY_FLAGS ENABLE_BOARD_FLAGS ENABLE_WEBM_AUDIO MIN_W MIN_H TEXT_ONLY
FORCED_ANON REQUIRE_SUBJECT ENABLE_PAINTERJS'''.split()

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path, nargs='?')
parser.add_argument('--fixture-only', action='store_true')
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()

def execute(code, payload):
    result = subprocess.run([args.php, '-d', 'memory_limit=64M', '-d', 'max_execution_time=5', '-r', code],
        input=json.dumps(payload).encode(), capture_output=True, timeout=10, check=True)
    assert not result.stderr, result.stderr.decode()
    assert len(result.stdout) < 65536
    return json.loads(result.stdout)

def data_source(cases):
    rows = []
    for case in cases:
        raw = case['expected'].get('meta_description')
        if not isinstance(raw, str):
            continue
        logical = case['logical_description']
        assert logical == html.unescape(raw.strip())
        if raw == logical:
            continue
        assert all(ord(c) >= 32 for c in raw + logical)
        fields = [json.dumps(value, ensure_ascii=False) for value in (case['board'], logical, raw)]
        rows.append(f'        ({fields[0]}, {fields[1]}) => {fields[2]},')
    rust = "//! Exact source encoding for unchanged imported descriptions. Operator edits\n//! remain their own literal text; this lookup never supplies template markup.\npub(super) fn description<'a>(board: &str, current: &'a str) -> &'a str {\n    match (board, current) {\n" + '\n'.join(rows) + '\n        _ => current,\n    }\n}\n'
    return subprocess.run(['rustfmt', '--edition', '2024', '--emit', 'stdout'],
        input=rust.encode(), capture_output=True, check=True, timeout=10).stdout.decode()

def save_data(cases):
    path = ROOT / 'apps/public/src/api/directory_data.rs'
    encoded = data_source(cases)
    if args.check:
        assert path.read_text() == encoded
    else:
        path.parent.mkdir(exist_ok=True)
        path.write_text(encoded)

if args.fixture_only:
    assert args.source is None and args.check
    fixture = json.loads((ROOT / 'fixtures/json-directory-reference.json').read_text())
    assert fixture['source']['imgboard.php'] == PIN and fixture['source']['lib/ini.php'] == INI_PIN
    block = fixture['generator_php']
    assert hashlib.sha256(block.encode()).hexdigest() == '13c2a5685842cb09a861b2d532176065ce73f69235d4b4d299f124a7fc1011dd'
    assert len(fixture['cases']) == 82
    for case in fixture['cases']:
        driver = "$recipe=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);foreach($recipe['constants'] as $key=>$value)define($key,$value);function get_board_flags_selector(){global $recipe;return $recipe['flags'];}\n" + block + '\nget_catalog_info();'
        assert execute(driver, case) == case['expected'], case['board']
    save_data(fixture['cases'])
    print('Executed 82 captured source JSON directory cases and checked description data.')
    raise SystemExit(0)

assert args.source is not None
source = args.source.resolve()
raw = (source / 'imgboard.php').read_bytes()
assert hashlib.sha256(raw).hexdigest() == PIN
assert hashlib.sha256((source / 'lib/ini.php').read_bytes()).hexdigest() == INI_PIN
text = raw.decode().replace('\r\n', '\n')
start = text.index('function get_catalog_info() {')
end = text.index('\n/**', start)
block = text[start:end].strip()
assert block.endswith('die(json_encode($arr));\n}')
flags = json.loads((ROOT / 'apps/public/tests/fixtures/board-flags.json').read_text())
board_flags = {item['board']: item for item in flags['boards']}
pins = {'imgboard.php': PIN, 'lib/ini.php': INI_PIN}
for name, expected in flags['files'].items():
    if name.startswith('config/') or name.startswith('lib/board_flags_'):
        assert hashlib.sha256((source / name).read_bytes()).hexdigest() == expected, name
        pins[name] = expected

program = r'''
$recipe=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
require $recipe['source'].'/lib/ini.php';
$constants=array_replace(parse_ini($recipe['source'].'/config/global_config.ini'),
 parse_ini($recipe['source'].'/config/categories/'.$recipe['category'].'.config.ini'),
 parse_ini($recipe['source'].'/config/boards/'.$recipe['board'].'.config.ini'));
$selected=[];
foreach ($recipe['keys'] as $key) {
 if (!array_key_exists($key,$constants)) continue;
 if (str_contains($constants[$key],'{{')) throw new Exception('Public field unexpectedly interpolates');
 $selected[$key]=evaluate($constants[$key]);
}
echo json_encode($selected,JSON_THROW_ON_ERROR);
'''


cases = []
for board in sorted(board_flags):
    ini = source / f'config/boards/{board}.config.ini'
    category = next(line.split('=',1)[1].strip() for line in ini.read_text().splitlines()
                    if line.startswith('CATEGORY'))
    selected = execute(program, {'source': str(source), 'board': board, 'category': category, 'keys': KEYS})
    policy = board_flags[board]
    table = flags['tables'][policy['type']]['selector'] if policy['enabled'] else {}
    # The generator's one helper has its own independently executed, pinned
    # source-table fixture. No application entry point, database or network runs.
    driver = r'''
$recipe=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
foreach ($recipe['constants'] as $key=>$value) define($key,$value);
function get_board_flags_selector() { global $recipe; return $recipe['flags']; }
''' + block + '\nget_catalog_info();'
    expected = execute(driver, {'constants': selected, 'flags': table})
    cases.append({'board': board, 'constants': selected, 'flags': table, 'expected': expected,
                  'logical_description': html.unescape(expected.get('meta_description', '').strip())})
result = {'source': pins, 'scope': 'Unchanged get_catalog_info and independently qualified flag selector; excludes boardlist SQL and title data.',
          'generator_php': block, 'cases': cases}
encoded = json.dumps(result, ensure_ascii=False, indent=2) + '\n'
target = ROOT / 'fixtures/json-directory-reference.json'
if args.check:
    assert target.read_text() == encoded
else:
    target.write_text(encoded)
save_data(cases)
print(f'Executed source JSON directory policies: {len(cases)}')
