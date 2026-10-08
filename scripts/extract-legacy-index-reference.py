"""Capture only pinned updating_index echo/protocol logic, without endpoint execution."""
import argparse
import hashlib
import json
import re
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
    'config/global_strings.ini': '3b3b60083db74fb1693af6f0824cccd6ec3ca446242474008cf9be474c65cd17',
}
sources = {}
for name, expected in hashes.items():
    raw = (args.source / name).read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('Audited index source changed.')
    sources[name] = raw.decode('utf8').replace('\r\n', '\n')
source = sources['imgboard.php']
start = source.index('function updating_index()')
function = source[start:source.index('function require_request_method(', start)]
message_match = re.findall(r'^S_UPDATING_INDEX = (.+)$', sources['config/global_strings.ini'], re.M)
if len(message_match) != 1 or message_match[0] != 'Updating index...':
    raise ValueError('Unexpected source message.')
message = message_match[0]
if len(function) > 1024 or function.count('echo ') != 1:
    raise ValueError('Unexpected updating_index boundary.')
rows = []
runtime = None
for board, domain in [('g', '4chan.org'), ('po', '4channel.org')]:
    # Domain mapping is a synthetic boundary, not a claim to execute L::d.
    program = (
        f"define('BOARD_DIR',{json.dumps(board)});define('S_UPDATING_INDEX',{json.dumps(message)});"
        f"class L{{static function d($board){{return {json.dumps(domain)};}}}}"
        + function
        + "$inputs=json_decode(stream_get_contents(STDIN),true,8,JSON_THROW_ON_ERROR);$rows=[];"
        "foreach($inputs as $referer){$_SERVER=['HTTP_REFERER'=>$referer];ob_start();"
        "updating_index();$body=ob_get_clean();if(strlen($body)>1024){throw new Exception('Unexpected body.');}"
        "$rows[]=['board'=>BOARD_DIR,'referer'=>$referer,'body'=>$body];}"
        "echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
    )
    if len(program) > 4096:
        raise ValueError('Harness exceeds audited bounds.')
    run = subprocess.run(
        [args.php, '-n', '-d', 'memory_limit=32M', '-d', 'max_execution_time=5', '-r', program],
        input=json.dumps(['', 'http://example.invalid/', 'https://example.invalid/',
                         'http://example.invalid/?next=HTTPS']).encode(),
        capture_output=True, timeout=10, check=True,
    )
    if run.stderr or len(run.stdout) > 8192:
        raise ValueError('Unexpected source index output.')
    result = json.loads(run.stdout)
    if len(result['cases']) != 4 or runtime not in (None, result['php']):
        raise ValueError('Incomplete cases or inconsistent runtime.')
    runtime = result['php']
    for row in result['cases']:
        body = row['body']
        match = re.fullmatch(
            r'<!doctype html><head><meta http-equiv="refresh" content="([0-9]+);URL=([^"]+)">'
            r'<title>([^<]+)</title></head><body><table style="([^"]+)"><td><strong>([^<]+)</strong></td></table>',
            body,
        )
        if match is None:
            raise ValueError('Unexpected source body shape.')
        seconds, target, title, style, text = match.groups()
        rows.append(row | {'domain_stub': domain, 'refresh_seconds': int(seconds),
                           'target': target, 'title': title, 'style': style, 'text': text})
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': hashes,
    'selected_sha256': {'updating_index': hashlib.sha256(function.encode()).hexdigest()},
    'source_message': message,
    'extractor_php': runtime,
    'scope': 'only original updating_index function echo/protocol logic and extracted global message; no endpoint, auth, database, rebuild, HTTP status or cache-header execution',
    'boundary_stubs': ['two fixed synthetic board/domain mappings replacing L::d',
                       'explicit Referer values including empty string, not missing-key diagnostics',
                       'bounded output capture; no source code rewritten'],
    'static_only_observation': 'updating_index contains no status/header/rebuild calls; the default endpoint and its HTTP/privacy/cache behavior were not dynamically exercised',
    'rewrite_adaptations': ['same-origin local board target, never Referer-derived scheme/domain',
                            'private no-store response policy is infrastructure, not source body evidence',
                            'external CSP-safe CSS replaces source inline table styling'],
    'cases': rows,
}
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
if len(rows) != 8 or len(data) > 32768:
    raise ValueError('Fixture exceeds audited bounds.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Legacy index reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({'cases': len(rows), 'php': runtime, 'selected_sha256': fixture['selected_sha256']}))
