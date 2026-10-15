#!/usr/bin/env python3
"""Run the pinned pure Twister normalizer; never load the challenge application."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE_SHA = '0789325aa9d83bff99480e8a713a28b862ccfbf84c2b8a9f0e652ae562860253'
METHOD_SHA = '1e3e789f1de8c67387de5b56fa4aa05ab852ff169e464fe949175ea26bd7e01a'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--php', default='php')
    parser.add_argument('--source', type=Path)
    args = parser.parse_args()
    raw = (ROOT / 'fixtures/challenge-answer-reference.json').read_bytes()
    assert len(raw) <= 16384
    fixture = json.loads(raw)
    assert fixture['source_sha256'] == SOURCE_SHA
    method = fixture['method']
    assert fixture['method_sha256'] == METHOD_SHA
    assert hashlib.sha256(method.encode()).hexdigest() == METHOD_SHA
    if args.source:
        source = (args.source / 'twister/twister_captcha.php').read_bytes()
        assert hashlib.sha256(source).hexdigest() == SOURCE_SHA
        assert method in source.decode().replace('\r\n', '\n')
    cases = fixture['cases']
    assert len(cases) == 20 and all(len(case['raw'].encode()) <= 128 for case in cases)
    program = '<?php\nclass OwnedChallengeReference {\n' + method + '''
}
$inputs=json_decode(stream_get_contents(STDIN),true,16,JSON_THROW_ON_ERROR);
$output=[];
foreach($inputs as $input) { $output[]=OwnedChallengeReference::normalizeReponseStr($input); }
echo json_encode($output,JSON_THROW_ON_ERROR);
'''
    with tempfile.TemporaryDirectory(prefix='owned-challenge-reference-') as directory:
        path = Path(directory) / 'oracle.php'
        path.write_text(program)
        result = subprocess.run(
            [args.php, '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0',
             '-d', 'memory_limit=32M', str(path)],
            input=json.dumps([case['raw'] for case in cases]), text=True,
            capture_output=True, check=True, timeout=10,
        )
    assert json.loads(result.stdout) == [case['normalized'] for case in cases]
    for case in cases:
        assert case['within_rust_bounds'] == (len(case['raw'].encode()) <= 64 and 4 <= len(case['normalized']) <= 6)
    print('20 pinned Twister answer cases passed. No challenge service or renderer was invoked.')


if __name__ == '__main__':
    main()
