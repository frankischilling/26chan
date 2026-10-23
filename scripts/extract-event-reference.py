#!/usr/bin/env python3
"""Execute isolated source event presentation helpers, not dormant controllers.

The score and net-worth reads are explicit scalar boundaries. No source entry
point, database connection, historical secret or account record is executed.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PINS = {
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'lib/like_score.php': 'f62362649585ec9956cf412c8de9174ebdf6cac6c9b46f0d1a41bc4a2f835992',
}
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path, nargs='?')
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
path = ROOT / 'fixtures/event-reference.json'
if args.source:
    sources = {}
    for name, pin in PINS.items():
        raw = (args.source / name).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == pin
        sources[name] = raw.decode().replace('\r\n', '\n')
    board = sources['imgboard.php']
    stock = board[board.index('function april_2024_get_stock_list('):board.index('function april_2024_get_stock_count(')].strip()
    start = board.index('function get_halloween_css_cls(')
    halloween = board[start:board.index('\n}', start) + 2]
    likes = sources['lib/like_score.php']
    perks = likes[likes.index('function like_get_perks_state('):likes.index('function like_build_perk_html(')].strip()
    functions = {'stock': stock, 'halloween': halloween, 'perks': perks}
else:
    assert args.check
    saved = json.loads(path.read_text())
    assert saved['source'] == PINS
    functions = saved['functions']
# Exact slices are independently pinned for fixture-only CI replay.
FUNCTION_PINS = {'stock': 'e4e75f6cbb79e3f944dd33374c9af15293b1b7caf039d0c2faecf221c649150c', 'halloween': '82ed2d7a552d1f523f2d6d0427250b79d389a7041d5b8a941bb0cd0d32601709', 'perks': 'cdfacd71c28936dee5b3da82b68507b3bf658a12855028815226d77ef32941e9'}
assert {name: hashlib.sha256(value.encode()).hexdigest() for name, value in functions.items()} == FUNCTION_PINS

driver = r'''
error_reporting(E_ALL);
set_error_handler(function($severity,$message){
 if($severity===E_WARNING && str_starts_with($message,'Undefined array key ')) return true;
 throw new Exception($message);
});
$input=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);
// The actual source score getter returns zero before its database lookup.
function like_get_user_score(){return 0;}
function april_2024_get_net_worth(){global $worth;return $worth;}
''' + '\n'.join(functions.values()) + r'''
$out=['stock'=>[], 'investor'=>[], 'halloween'=>[], 'perks'=>[], 'options'=>[]];
foreach($input['stock'] as $value){$out['stock'][]=['value'=>$value,
 'stock'=>april_2024_get_stock_from_s4p($value),
 'class'=>april_2024_get_post_cls($value), 'badge'=>april_2024_get_name_badge($value)];}
foreach($input['investor'] as $worth){$out['investor'][]=['value'=>$worth,'name'=>april_2024_get_name()];}
foreach($input['halloween'] as $value){$out['halloween'][]=['value'=>$value,'class'=>get_halloween_css_cls($value)];}
foreach($input['perks'] as $value){$out['perks'][]=['score'=>$value,'state'=>like_get_perks_state($value),'unlocked'=>like_get_perks_state($value,true)];}
foreach($input['options'] as $value){$out['options'][]=['value'=>$value,'result'=>like_parse_options_field($value)];}
echo json_encode($out,JSON_THROW_ON_ERROR);
'''
inputs = {
    'stock': [-1, 0, 9999, *range(10000, 10020), 32767, 2147483647],
    'investor': [-1, 0, *[value + offset for value in [500, 1500, 5000, 50000, 500000, 2000000, 5000000] for offset in [-1, 0, 1]]],
    'halloween': [-1, 0, *[value + offset for value in [25, 50, 100, 200, 500, 1000] for offset in [-1, 0, 1]]],
    'perks': [-1, 0, 1, 749, 750, 999, 1000, 2147483647],
    'options': ['', 'smiley', 'sad smiley', ' smiley  sad ', 'showscore smiley', 'SMILEY', 'unknown sad', 'party', 'dino', 'spooky', 'smiley\tsad', 'smiley\nsad', 'animal food', 'joy', 'frog', 'smiley' + ' ' * 94, 'smiley' + ' ' * 95, '<script>'],
}
result = subprocess.run([args.php, '-d', 'memory_limit=32M', '-d', 'max_execution_time=5', '-r', driver],
                        input=json.dumps(inputs).encode(), capture_output=True, check=True, timeout=10)
assert not result.stderr, result.stderr.decode()
fixture = {'source': PINS, 'scope': __doc__, 'functions': functions, 'cases': json.loads(result.stdout)}
encoded = json.dumps(fixture, ensure_ascii=False, indent=2) + '\n'
if args.check:
    assert path.read_text() == encoded
else:
    path.write_text(encoded)
print(f'Executed {sum(len(cases) for cases in fixture["cases"].values())} source event helper cases.')
