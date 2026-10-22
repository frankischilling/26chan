#!/usr/bin/env python3
"""Extract the pinned EXIF table branch without executing the application."""
import argparse
import hashlib
import json
import re
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('source', type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
raw = (args.source / 'imgboard.php').read_bytes()
assert hashlib.sha256(raw).hexdigest() == 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
text = raw.decode().replace('\r\n', '\n')
start = text.index('\tif( ENABLE_EXIF == 1 && $exif ) {')
end = text.index('\n\t$name  = preg_replace', start)
strings = (args.source / 'config/global_strings.ini').read_bytes()
line = next(line for line in strings.decode().splitlines() if line.startswith('S_EXIF_TOGGLE = '))
result = {
    'source_sha256': hashlib.sha256(raw).hexdigest(),
    'strings_sha256': hashlib.sha256(strings).hexdigest(),
    'toggle': line.split(' = ', 1)[1],
    'table_php': text[start:end].strip(),
    'ordering': {
        'extraction': text.index('shell_exec( "/usr/local/bin/exiftags $dest" )'),
        'table': start,
        'capcode_cleanup': text.index("if( $ma != 'None' && STRIP_EXIF_ON_CAPCODE"),
        'append': text.index('$com .= $exiftext;'),
        'wordfilter': text.index('if( WORD_FILT && $word_filters_enabled )'),
    },
}
assert list(result['ordering'].values()) == sorted(result['ordering'].values())
result['styles'] = {}
for name in ['yotsubanew', 'yotsubluenew', 'futabanew', 'burichannew', 'photon', 'tomorrow', 'yotsubamobile', 'yotsubluemobile']:
    path = args.source / 'css' / (name + '.css')
    data = path.read_bytes()
    rules = re.findall(r'table\.exif(?: td(?: b)?)?\s*\{[^}]*\}', data.decode())
    assert len(rules) == 3
    result['styles'][name] = {'sha256': hashlib.sha256(data).hexdigest(), 'rules': [' '.join(rule.split()) for rule in rules]}
encoded = json.dumps(result, indent=2, ensure_ascii=False) + '\n'
path = Path('fixtures/exif-comment-source.json')
if args.check:
    assert path.read_text() == encoded
else:
    path.write_text(encoded)
print('EXIF table source and extraction-before-capcode-cleanup order checked')
