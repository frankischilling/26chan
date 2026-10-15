#!/usr/bin/env python3
"""Run the captured frontpage admission helper with a read-only database double."""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
FUNCTION_SHA = 'e28923c02be00d90a28b3131adf7ccf2fd5ebe7383730f8ff03a64f95e3cf81e'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--php', default='php')
    args = parser.parse_args()
    fixture = json.loads((ROOT / 'fixtures/homepage-promotion-reference.json').read_text())
    sources = json.loads((ROOT / 'fixtures/homepage-reference.json').read_text())
    assert fixture['version'] == 1
    assert sources['source_files'][fixture['source_path']] == fixture['source_sha256']
    assert hashlib.sha256(fixture['function'].encode()).hexdigest() == FUNCTION_SHA == fixture['function_sha256']
    assert len(fixture['cases']) == 14
    program = '''<?php
namespace OwnedFrontpage;
function mysql_global_call($query) {
    global $case;
    if ($query !== "SELECT COUNT(*) from reports where board='g' and no='123'") {
        throw new \\Exception('Unexpected fixture query');
    }
    return $case['reports'];
}
function mysql_fetch_array($result) { return array($result); }
''' + fixture['function'] + '''
$cases = json_decode(stream_get_contents(STDIN), true, 32, JSON_THROW_ON_ERROR);
$out = array();
foreach ($cases as $case) { $out[] = check_post($case['post']); }
echo json_encode($out, JSON_THROW_ON_ERROR);
'''
    env = {'PATH': os.environ.get('PATH', '/usr/bin:/bin'), 'LC_ALL': 'C'}
    if 'LD_LIBRARY_PATH' in os.environ:
        env['LD_LIBRARY_PATH'] = os.environ['LD_LIBRARY_PATH']
    with tempfile.TemporaryDirectory(prefix='owned-frontpage-oracle-') as directory:
        source = pathlib.Path(directory) / 'oracle.php'
        source.write_text(program)
        result = subprocess.run([args.php, '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0', str(source)], input=json.dumps(fixture['cases']),
                                text=True, capture_output=True, check=True, timeout=10, env=env)
    assert json.loads(result.stdout) == [case['expected'] for case in fixture['cases']]
    print('14 pinned PHP frontpage admission decisions passed; no database accessed')


if __name__ == '__main__':
    main()
