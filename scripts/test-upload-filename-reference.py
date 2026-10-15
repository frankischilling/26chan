#!/usr/bin/env python3
"""Check upload-name expectations against the pinned, isolated source sanitizer."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE_SHA = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
FUNCTION_SHA = 'b53f07ac6ff9b0e8007705caa1406976367342efad6c3b299d608f2936369730'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--php', default='php')
    parser.add_argument('--source', type=Path)
    args = parser.parse_args()
    reference = json.loads((ROOT / 'fixtures/source-upload-filenames.json').read_text())
    assert reference['source_sha256'] == SOURCE_SHA
    assert reference['function_sha256'] == FUNCTION_SHA
    assert hashlib.sha256(reference['function'].encode()).hexdigest() == FUNCTION_SHA
    if args.source:
        raw = (args.source / 'imgboard.php').read_bytes()
        assert hashlib.sha256(raw).hexdigest() == SOURCE_SHA
        assert reference['function'] in raw.decode().replace('\r\n', '\n')
    # Only this reviewed string function executes. Legacy magic quotes are
    # unavailable; HTML permission is false. No application bootstrap or I/O.
    prelude = '''$case = json_decode(stream_get_contents(STDIN), true, 512, JSON_THROW_ON_ERROR);
define('BOARD_DIR', $case['board']);
define('CODE_TAGS', $case['code']);
define('SJIS_TAGS', $case['sjis']);
function get_magic_quotes_gpc() { return false; }
'''
    invoke = '''$name = preg_replace('/\\xe2\\x80(\\xae|\\xad|\\x8f|\\x8e)/', '', $case['input']);
$value = html_entity_decode(sanitize_text($name), ENT_QUOTES, 'UTF-8');
echo json_encode($value, JSON_THROW_ON_ERROR);
'''
    for case in reference['cases']:
        result = subprocess.run(
            [args.php, '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0',
             '-d', 'memory_limit=16M', '-r', prelude + reference['function'] + invoke],
            input=json.dumps(case), text=True, capture_output=True, timeout=5, check=True)
        assert json.loads(result.stdout) == case['expected'], case
    print(f"All {len(reference['cases'])} source upload-name cases match the captured PHP sanitizer.")


if __name__ == '__main__':
    main()
