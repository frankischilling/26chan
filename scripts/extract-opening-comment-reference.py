#!/usr/bin/env python3
"""Pin the final OP rule and effective board policy without running the application."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
raw = (args.source / 'imgboard.php').read_bytes()
assert hashlib.sha256(raw).hexdigest() == 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
source = raw.decode().replace('\r\n', '\n')
start = source.index('\tif (!$resto && COM_REGEX) {')
end = source.index('\n\t//post text is now completely created', start)
block = source[start:end].strip()
spec = importlib.util.spec_from_file_location('board_reference', Path(__file__).with_name('extract-board-reference.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.POLICY_KEYS.update({'COM_REGEX', 'S_INVALID_COM'})
base = module.policy(args.source / 'config/global_config.ini')
strings = module.policy(args.source / 'config/global_strings.ini')
policies = {}
pins = {'imgboard.php': hashlib.sha256(raw).hexdigest()}
for board in sorted((args.source / 'config/boards').glob('*.config.ini')):
    slug = board.name.removesuffix('.config.ini')
    own = module.policy(board)
    values = dict(base)
    values.update(strings)
    category = args.source / 'config/categories' / (own['CATEGORY'] + '.config.ini')
    values.update(module.policy(category))
    values.update(own)
    pattern = values.get('COM_REGEX', 'no')
    if pattern.lower() == 'no':
        pattern = ''
    policies[slug] = {'pattern': pattern, 'refusal': values['S_INVALID_COM']}
    for path in [board, category, args.source / 'config/global_config.ini', args.source / 'config/global_strings.ini']:
        pins[str(path.relative_to(args.source))] = hashlib.sha256(path.read_bytes()).hexdigest()
postfilter = (args.source / 'lib/postfilter.php').read_bytes()
pins['lib/postfilter.php'] = hashlib.sha256(postfilter).hexdigest()
filter_text = postfilter.decode().replace('\r\n', '\n')
quiet_start = filter_text.index("      if ($matched_filter['quiet']) {")
quiet_end = filter_text.index('\n      }', quiet_start) + len('\n      }')
quiet_exit = filter_text[quiet_start:quiet_end]
assert 'show_post_successful_fake($resto);' in quiet_exit and 'die();' in quiet_exit
result = {'source': pins, 'gate_php': block, 'quiet_filter_exit_php': quiet_exit, 'boards': policies}
encoded = json.dumps(result, indent=2, ensure_ascii=False) + '\n'
path = Path('fixtures/opening-comment-source.json')
if args.check:
    assert path.read_text() == encoded
else:
    path.write_text(encoded)
print(f'Opening comment source policies: {len(policies)}')
