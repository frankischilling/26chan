#!/usr/bin/env python3
"""Evaluate the supplied board-flag tables and INI policy with PHP."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--rust', type=Path)
parser.add_argument('--client', type=Path)
parser.add_argument('--migration', type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
source = args.source.resolve()
reference = json.loads(Path('fixtures/board-reference.json').read_text(encoding='utf-8'))
pins = dict(reference['files'])
pins.update({'lib/ini.php': '05a89bb56d627f9f5d0a07f3200a3efc9fbe50e6d84eb7e68e86d012fb318567', 'lib/board_flags_pol.php': 'b624851027b8a1bdbd124545318e99c42a424c4c6a30dedbbd07697235210b9f', 'lib/board_flags_mlp.php': 'd5fc42a0b7fca475ea0664dbaa9e9d0d73a3e09621e8d20cd87eca235195b805', 'lib/board_flags_lgbt.php': '63a23bb5305e1c2ce1cdd943cd1a1f7062034a946b0a195c4fd76b18ba338f6e', 'lib/board_flags_test.php': 'c61894ce2a08ae180b056fb73c5c9410fbd7c83a956b73c35ccc1d6cf3c5327b'})
for name, expected in pins.items():
    assert hashlib.sha256((source / name).read_bytes()).hexdigest() == expected, name

runtime = None
def execute(program, recipe):
    global runtime
    result = subprocess.run(['php', '-d', 'memory_limit=64M', '-d', 'max_execution_time=5', '-r', program],
        input=json.dumps(recipe).encode(), capture_output=True, timeout=10, check=True)
    assert not result.stderr and len(result.stdout) < 32768
    row = json.loads(result.stdout)
    version = row.pop('php')
    assert runtime is None or runtime == version
    runtime = version
    return row

tables = {}
for kind in ['pol', 'mlp', 'lgbt', 'test']:
    tables[kind] = execute(r'''
    $recipe=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
    require $recipe['file'];
    echo json_encode(['php'=>PHP_VERSION,'display'=>get_board_flags_array(),
      'selector'=>get_board_flags_selector(),'unknown'=>board_flag_code_to_name('INVALID')],JSON_THROW_ON_ERROR);
    ''', {'file': str(source / f'lib/board_flags_{kind}.php')})
    table = tables[kind]
    table['selector_order'] = list(table['selector'])
    assert table['unknown'] == 'None' and set(table['display']) == set(table['selector'])
    assert all(code.isascii() and code.isalnum() and code.upper() == code and 2 <= len(code) <= 3
        and 1 <= len(name.encode()) <= 100 for code, name in table['display'].items())

boards = []
for board in reference['boards']:
    row = execute(r'''
    $recipe=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
    require $recipe['source'].'/lib/ini.php';
    $constants=array_replace(parse_ini($recipe['source'].'/config/global_config.ini'),
      parse_ini($recipe['source'].'/config/categories/'.$recipe['category'].'.config.ini'),
      parse_ini($recipe['source'].'/config/boards/'.$recipe['board'].'.config.ini'));
    $type=isset($constants['BOARD_FLAGS_TYPE'])?evaluate($constants['BOARD_FLAGS_TYPE']):'';
    echo json_encode(['php'=>PHP_VERSION,'board'=>$recipe['board'],
      'enabled'=>(bool)evaluate($constants['ENABLE_BOARD_FLAGS']),
      'type'=>$type?:$recipe['board'],'css_version'=>(int)evaluate($constants['CSS_VERSION_BOARD_FLAGS'])],JSON_THROW_ON_ERROR);
    ''', {'source': str(source), 'board': board['slug'], 'category': board['source_policy']['CATEGORY']})
    assert not row['enabled'] or row['type'] in tables
    boards.append(row)

fixture = {'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22', 'files': pins,
    'extractor_php': runtime, 'scope': 'actual flag dictionaries, selector labels/order and all 82 INI policies',
    'excludes': ['complete posting execution', 'historical sprite pixel identity', 'country provider data'],
    'tables': tables, 'boards': boards}
outputs = [(args.output, json.dumps(fixture, ensure_ascii=False, indent=2) + '\n')]
literal = lambda value: json.dumps(value, ensure_ascii=False)
rust = ['//! Source display labels and menu labels have separate meanings.',
    '#[derive(Clone, Copy, Debug)]', 'pub struct Flag { pub code: &\'static str, pub display: &\'static str, pub selector: &\'static str }']
for kind, table in tables.items():
    rust += [f'const {kind.upper()}: &[Flag] = &[']
    rust += ['Flag { code: ' + literal(code) + ', display: ' + literal(table['display'][code])
        + ', selector: ' + literal(label) + ' },' for code, label in table['selector'].items()]
    rust += ['];']
rust += ['pub fn flags(kind: &str) -> &\'static [Flag] { match kind { '
    + ','.join(f'{literal(kind)} => {kind.upper()}' for kind in tables) + ', _ => &[] } }',
    'pub fn flag(kind: &str, code: &str) -> Option<Flag> { flags(kind).iter().copied().find(|flag| flag.code == code) }']
if args.rust:
    # Keep generation independent of the host's rustfmt installation.
    result = subprocess.run(['rustfmt', '--edition', '2024', '--emit', 'stdout'], input=('\n'.join(rust)+'\n').encode(),
        capture_output=True, timeout=10, check=True)
    outputs.append((args.rust, result.stdout.decode()))
if args.client:
    codes = {kind: ' ' + ' '.join(table['display']).lower() + ' ' for kind, table in tables.items()}
    outputs.append((args.client, '// Fixed code membership from the source flag tables.\nexport const boardFlagCodes = Object.freeze('
        + json.dumps(codes, separators=(',', ':')) + ');\n'))
if args.migration:
    sql = lambda value: "'" + value.replace("'", "''") + "'"
    array = lambda values: 'ARRAY[' + ','.join(map(sql, values)) + ']::text[]'
    label_cases = ' '.join('WHEN ' + sql(kind) + ' THEN CASE code ' + ' '.join('WHEN ' + sql(code) + ' THEN ' + sql(name)
        for code, name in table['display'].items()) + ' END' for kind, table in tables.items())
    code_cases = ' '.join('WHEN ' + sql(kind) + ' THEN ' + array(table['display']) for kind, table in tables.items())
    migration = f'''-- Source flag types, finite choices and distinct persisted display labels.
CREATE FUNCTION content.board_flag_label(kind text,code text) RETURNS text
LANGUAGE sql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,pg_temp AS $$ SELECT CASE kind {label_cases} END $$;
CREATE FUNCTION content.board_flag_codes(kind text) RETURNS text[]
LANGUAGE sql IMMUTABLE PARALLEL SAFE SET search_path=pg_catalog,pg_temp AS $$ SELECT CASE kind {code_cases} ELSE '{{}}'::text[] END $$;
REVOKE ALL ON FUNCTION content.board_flag_label(text,text),content.board_flag_codes(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION content.board_flag_label(text,text) TO board_public,board_staff,board_attachment_owner,board_staff_post_owner;
ALTER TABLE content.boards ADD COLUMN board_flag_type text NOT NULL DEFAULT 'pol'
  CHECK (board_flag_type IN ('pol','mlp','lgbt','test'));
ALTER TABLE content.boards DROP CONSTRAINT boards_board_flags_check;
ALTER TABLE content.boards ADD CONSTRAINT boards_board_flags_check CHECK (
  CASE WHEN cardinality(board_flags)=0 THEN true
  WHEN array_ndims(board_flags)=1 AND array_lower(board_flags,1)=1 THEN
    cardinality(board_flags)<=83 AND array_position(board_flags,NULL) IS NULL
    AND board_flags <@ content.board_flag_codes(board_flag_type)
  ELSE false END);
ALTER TABLE content.posts ADD COLUMN board_flag_type text NOT NULL DEFAULT 'pol'
  CHECK (board_flag_type IN ('pol','mlp','lgbt','test'));
ALTER TABLE content.posts DROP CONSTRAINT posts_board_flag_check;
ALTER TABLE content.posts ADD CONSTRAINT posts_board_flag_check CHECK (
  board_flag IS NULL OR content.board_flag_label(board_flag_type,board_flag) IS NOT NULL);
GRANT SELECT(board_flag_type) ON content.boards TO board_attachment_owner,board_staff_post_owner;
CREATE OR REPLACE FUNCTION content.apply_post_flag() RETURNS trigger
LANGUAGE plpgsql SET search_path=pg_catalog,pg_temp AS $$
DECLARE v_geo boolean; v_flags text[]; v_selected text; v_type text;
BEGIN
  NEW.board_flag_type := 'pol';
  IF NEW.capcode IS NOT NULL THEN NEW.country:=NULL; NEW.country_name:=NULL; NEW.board_flag:=NULL; NEW.flag_name:=NULL; RETURN NEW; END IF;
  SELECT country_flags,board_flags,board_flag_type INTO v_geo,v_flags,v_type FROM content.boards WHERE slug=NEW.board FOR SHARE;
  IF NOT FOUND THEN RAISE EXCEPTION 'Board is unavailable.' USING ERRCODE='23503'; END IF;
  v_selected := coalesce(nullif(current_setting('board.flag',true),''),'0');
  NEW.country := NULL; NEW.country_name := NULL; NEW.board_flag := NULL; NEW.flag_name := NULL;
  IF v_selected <> '0' THEN
    IF NOT v_selected=ANY(v_flags) OR content.board_flag_label(v_type,v_selected) IS NULL THEN
      RAISE EXCEPTION 'Invalid board flag.' USING ERRCODE='23514';
    END IF;
    NEW.board_flag := v_selected; NEW.board_flag_type := v_type;
    NEW.flag_name := content.board_flag_label(v_type,v_selected);
  ELSIF v_geo THEN
    NEW.country := nullif(current_setting('board.country',true),'');
    NEW.country_name := nullif(current_setting('board.country_name',true),'');
    IF NEW.country IS NULL OR NEW.country_name IS NULL THEN
      RAISE EXCEPTION 'Country flags are unavailable.' USING ERRCODE='23514';
    END IF;
  END IF;
  RETURN NEW;
END $$;
UPDATE content.boards b SET board_flag_type=policy.kind,board_flags=policy.codes
FROM (VALUES
'''
    rows = []
    for board in boards:
        kind = board['type'] if board['type'] in tables else 'pol'
        codes = list(tables[kind]['selector']) if board['enabled'] else []
        rows.append('(' + sql(board['board']) + ',' + sql(kind) + ',' + array(codes) + ')')
    migration += ',\n'.join(rows) + ') policy(slug,kind,codes) WHERE b.slug=policy.slug;\n'
    outputs.append((args.migration, migration))
for path, text in outputs:
    if args.check:
        assert path.read_bytes() == text.encode(), f'{path} differs from the source projection'
    else:
        path.write_bytes(text.encode())
print(f'Verified {len(boards)} board policies and {sum(len(t["display"]) for t in tables.values())} flag definitions with PHP {runtime}.')
