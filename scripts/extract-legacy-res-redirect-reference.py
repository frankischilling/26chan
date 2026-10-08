"""Record pinned public resredir target/status selection without database or headers."""
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
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'config/global_config.ini': '8bebeedec119b30559cba4fdccfef416653294cf62118d10288434be99a3034d',
}
sources = {}
for name, expected in hashes.items():
    raw = (args.source / name).read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('Audited redirect source changed.')
    sources[name] = raw.decode('utf8').replace('\r\n', '\n')
source = sources['imgboard.php']
start = source.index('function resredir( $res, $delete = 0, $no_exit = false ) {')
protocol_start = source.index('if (!$_SERVER["HTTP_REFERER"])', start)
protocol = source[protocol_start:source.index('$res = (int)$res;', protocol_start)]
target_start = source.index('if( !JANITOR_BOARD ) {', start)
target = source[target_start:source.index('//mysql_board_unlock();', target_start)]
if len(protocol) > 512 or len(target) > 1536:
    raise ValueError('Selected redirect exceeds audited bounds.')
if target.count('header(') != 2 or target.count('http_response_code(') != 1 or target.count('error(') != 1:
    raise ValueError('Unexpected effect boundary.')
selected_hashes = {name: hashlib.sha256(value.encode()).hexdigest()
                   for name, value in [('protocol', protocol), ('target_and_status', target)]}
# Only these four effect calls are substituted. No query/cast/deletion branch,
# whole endpoint, source error renderer, authentication or request extraction runs.
recorded = target.replace('header(', 'record_header(').replace(
    'http_response_code(', 'record_status(').replace('error(', 'record_error(')
targets = [
    ('op', '42', '0'), ('reply', '43', '42'),
    ('large_op', '9007199254740993', '0'),
    ('large_reply', '9223372036854775807', '9007199254740993'),
    ('missing', '0', '0'),
]
recipes = [{'kind': kind, 'post': post, 'parent': parent, 'referer': referer}
           for kind, post, parent in targets
           for referer in ['', 'https://boards.4chan.org/g/', 'http://boards.4chan.org/g/',
                           'http://example.invalid/path?next=HTTPS']]
program = (
    "define('JANITOR_BOARD',false);define('BOARD_DIR','g');define('PHP_EXT2','');define('S_NOTHREADERR','Missing');"
    "class L{static function d($board){return '4chan.org';}}"
    "class MissingTarget extends Exception{}"
    "function record_header($value,$replace=true,$code=null){global $headers,$status;$headers[]=$value;if($code!==null){$status=$code;}}"
    "function record_status($value){global $status;$status=$value;}"
    "function record_error($message,$dest){throw new MissingTarget();}"
    "$recipes=json_decode(stream_get_contents(STDIN),true,16,JSON_THROW_ON_ERROR);$rows=[];"
    "foreach($recipes as $recipe){$_SERVER=['HTTP_REFERER'=>$recipe['referer']];"
    "$no=$recipe['post'];$resto=$recipe['parent'];$dest=null;$headers=[];$status=200;"
    "ob_start();try{" + protocol + recorded + "}catch(MissingTarget $e){}"
    "$body=ob_get_clean();if(strlen($body)>512){throw new Exception('Unexpected body.');}"
    "$rows[]=$recipe+['status'=>$status,'headers'=>$headers,'body'=>$body];}"
    "echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
)
if len(program) > 8192:
    raise ValueError('Harness exceeds audited bounds.')
run = subprocess.run(
    [args.php, '-n', '-d', 'memory_limit=32M', '-d', 'max_execution_time=5', '-r', program],
    input=json.dumps(recipes).encode(), capture_output=True, timeout=10, check=True,
)
if run.stderr or len(run.stdout) > 32768:
    raise ValueError('Unexpected redirect output.')
result = json.loads(run.stdout)
if len(result['cases']) != 20:
    raise ValueError('Missing redirect cases.')
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': hashes, 'selected_sha256': selected_hashes, 'extractor_php': result['php'],
    'scope': 'public resredir protocol and target/header/status selection only, with synthetic lookup results represented as exact decimal strings; no PHP input coercion, SQL lookup, deletion redirect, private board authorization or original error rendering',
    'boundary_stubs': ['fixed public g board, empty configured PHP_EXT2, and synthetic domain resolver',
                       'synthetic post/parent lookup results; no database functions selected',
                       'two header calls and one status call replaced by recorders',
                       'source error call replaced by caught MissingTarget exception',
                       'bounded captured meta-refresh output; no process exit'],
    'rewrite_adaptations': ['local same-origin Location instead of absolute Referer-derived scheme/domain',
                            'strict canonical positive signed-64-bit query parsing, outside this source fixture',
                            'existing public visibility and private-board protections remain authoritative'],
    'cases': result['cases'],
}
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
if len(data) > 65536:
    raise ValueError('Fixture exceeds audited bounds.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Legacy redirect reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({'cases': 20, 'php': result['php'], 'selected_sha256': selected_hashes}))
