"""Execute supplied INI and custom-spoiler projections without database initialization."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--check', action='store_true')
parser.add_argument('--migration', type=Path)
parser.add_argument('--thumbnail-migration', type=Path)
args = parser.parse_args()
source = args.source.resolve()
board_reference = json.loads(Path('fixtures/board-reference.json').read_text(encoding='utf-8'))
pins = dict(board_reference['files'])
pins.update({
    'lib/ini.php': '05a89bb56d627f9f5d0a07f3200a3efc9fbe50e6d84eb7e68e86d012fb318567',
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'json.php': '18ccc5ea60fdfff5aaebd4648288e2970fab5329bd23d121edf359da3ab93868',
    'catalog.php': '9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946',
})
for name, expected in pins.items():
    assert hashlib.sha256((source / name).read_bytes()).hexdigest() == expected, name
bodies = {name: (source / name).read_text(encoding='utf-8') for name in ['imgboard.php', 'json.php', 'catalog.php']}
catalog = bodies['imgboard.php'][bodies['imgboard.php'].index('function get_catalog_info()'):]
board_projection = catalog[catalog.index('  if (SPOILERS) {'):catalog.index('  if (DISP_ID) {')].strip()
metadata_projection = re.search(r"if \(SPOILERS\) \{\s*\$ary\['custom_spoiler'\] = \(int\)SPOILER_NUM;\s*\}", bodies['json.php']).group(0)
post_projection = re.search(r"if\( !\$banskip && SPOILERS && !\$var\['resto'\] \) \$var\['custom_spoiler'\] = \(int\)SPOILER_NUM;", bodies['json.php']).group(0)
catalog_projection = re.search(r"if\( SPOILERS \) \$catalogjson\['custom_spoiler'\] = \(int\)SPOILER_NUM;", bodies['catalog.php']).group(0)
assert len(board_projection) < 250 and len(metadata_projection) < 150 and len(post_projection) < 150 and len(catalog_projection) < 150
program = r'''
$recipe = json_decode(stream_get_contents(STDIN), true, 32, JSON_THROW_ON_ERROR);
require $recipe['source'] . '/lib/ini.php';
$constants = array_replace(parse_ini($recipe['source'].'/config/global_config.ini'),
    parse_ini($recipe['source'].'/config/categories/'.$recipe['category'].'.config.ini'),
    parse_ini($recipe['source'].'/config/boards/'.$recipe['board'].'.config.ini'));
$enabled = evaluate($constants['SPOILERS']);
$count = evaluate($constants['SPOILER_NUM']);
if (isset($recipe['enabled'])) $enabled = $recipe['enabled'];
if (isset($recipe['count'])) $count = $recipe['count'];
define('SPOILERS', $enabled); define('SPOILER_NUM', $count);
$arr=[]; BOARD_PROJECTION
$ary=[]; METADATA_PROJECTION
$catalogjson=[]; CATALOG_PROJECTION
$posts=[];
foreach ([0,17] as $resto) foreach ([false,true] as $banskip) {
    $var=['resto'=>$resto]; POST_PROJECTION
    $posts[]=['resto'=>$resto,'banskip'=>$banskip,'custom_spoiler'=>$var['custom_spoiler']??null];
}
$thumbnails=[];
for ($seed=0;$seed<256;$seed++) { mt_srand($seed); $thumbnails[]=evaluate($constants['SPOILER_THUMB']); }
$thumbnails=array_values(array_unique($thumbnails)); sort($thumbnails);
echo json_encode(['php'=>PHP_VERSION,'board'=>$recipe['board'],'enabled'=>(bool)$enabled,'count'=>(int)$count,
    'thumbnail_expression'=>$constants['SPOILER_THUMB'],'source_html_urls'=>$thumbnails,
    'board_spoilers'=>$arr['spoilers']??null,'board_custom_spoilers'=>$arr['custom_spoilers']??null,
    'op_metadata_custom_spoiler'=>$ary['custom_spoiler']??null,'catalog_html_custom_spoiler'=>$catalogjson['custom_spoiler']??null,
    'posts'=>$posts],JSON_THROW_ON_ERROR);
'''
for marker, projection in [('BOARD_PROJECTION', board_projection), ('METADATA_PROJECTION', metadata_projection), ('CATALOG_PROJECTION', catalog_projection), ('POST_PROJECTION', post_projection)]:
    program = program.replace(marker, projection)

runtime = None
def execute(recipe):
    global runtime
    result = subprocess.run(['php', '-d', 'memory_limit=64M', '-d', 'max_execution_time=5', '-r', program],
        input=json.dumps(recipe).encode(), capture_output=True, timeout=10, check=True)
    assert not result.stderr and len(result.stdout) < 16384
    row = json.loads(result.stdout)
    assert runtime is None or runtime == row['php']
    runtime = row.pop('php')
    assert 0 <= row['count'] <= 64
    names = sorted(set(re.findall(r'spoiler(?:-[a-z0-9]+)?\.png', row['thumbnail_expression'])))
    assert sorted(url.rsplit('/', 1)[-1] for url in row['source_html_urls']) == names
    assert all(url.startswith('//s.4cdn.org/image/') for url in row['source_html_urls'])
    return row

boards = [execute({'source': str(source), 'board': board['slug'], 'category': board['source_policy']['CATEGORY']}) for board in board_reference['boards']]
cases = [execute({'source': str(source), 'board': 'a', 'category': 'ws', 'enabled': enabled, 'count': count})
         for enabled in [False, True] for count in [0, 1, 6, 64]]
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': pins, 'extractor_php': runtime,
    'scope': 'actual INI evaluation, custom-spoiler board/OP/catalog projections and finite source thumbnail choices',
    'boundary_stubs': ['synthetic OP/reply identifiers', 'banned-post projection bit', 'eight explicit enabled/count policy cases'],
    'excludes': ['complete source posting/rendering', 'historical CDN pixel identity', 'cross-board native rendering'],
    'boards': boards, 'policy_cases': cases,
}
encoded = json.dumps(fixture, ensure_ascii=False, indent=2) + '\n'
rows = [f"('{board['board']}',{board['count']})" for board in boards]
migration = (
    '-- Source SPOILER_NUM counts from all pinned board configurations.\n'
    '-- Keep existing spoiler policy, posts, attachments and runtime grants unchanged.\n'
    'ALTER TABLE content.boards ADD COLUMN custom_spoiler_count integer NOT NULL DEFAULT 0\n'
    '  CHECK (custom_spoiler_count BETWEEN 0 AND 64);\n'
    'UPDATE content.boards b SET custom_spoiler_count=policy.count\n'
    'FROM (VALUES\n' + ',\n'.join(rows) + ') policy(slug,count) WHERE b.slug=policy.slug;\n'
)
thumbnail_names = sorted({url.rsplit('/', 1)[-1] for board in boards for url in board['source_html_urls']})
thumbnail_rows = ["('%s',ARRAY[%s]::text[])" % (board['board'], ','.join("'%s'" % url.rsplit('/', 1)[-1] for url in board['source_html_urls'])) for board in boards]
thumbnail_migration = (
    '-- Source SPOILER_THUMB choices; independent of SPOILER_NUM and spoiler enablement.\n'
    'ALTER TABLE content.boards ADD COLUMN spoiler_thumbnail_assets text[] NOT NULL DEFAULT ARRAY[\'spoiler.png\']::text[]\n'
    '  CHECK (array_ndims(spoiler_thumbnail_assets)=1 AND array_lower(spoiler_thumbnail_assets,1)=1\n'
    '    AND cardinality(spoiler_thumbnail_assets) BETWEEN 1 AND 64\n'
    '    AND array_position(spoiler_thumbnail_assets,NULL) IS NULL\n'
    '    AND spoiler_thumbnail_assets <@ ARRAY[' + ','.join("'%s'" % name for name in thumbnail_names) + ']::text[]);\n'
    'UPDATE content.boards b SET spoiler_thumbnail_assets=policy.assets\n'
    'FROM (VALUES\n' + ',\n'.join(thumbnail_rows) + ') policy(slug,assets) WHERE b.slug=policy.slug;\n'
)
if args.check:
    assert args.output.read_text(encoding='utf-8') == encoded, 'Recorded custom-spoiler reference changed.'
    if args.migration:
        assert args.migration.read_bytes() == migration.encode(), 'Recorded custom-spoiler migration changed.'
    if args.thumbnail_migration:
        assert args.thumbnail_migration.read_bytes() == thumbnail_migration.encode(), 'Recorded thumbnail policy migration changed.'
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(encoded, encoding='utf-8')
    if args.migration:
        args.migration.write_bytes(migration.encode())
    if args.thumbnail_migration:
        args.thumbnail_migration.write_bytes(thumbnail_migration.encode())
print(f'{len(boards)} board configurations and {len(cases)} custom-spoiler policy cases executed with PHP {runtime}.')
