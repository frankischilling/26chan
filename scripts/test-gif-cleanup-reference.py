#!/usr/bin/env python3
"""Execute only the pinned GIF cleanup decision function with bounded I/O doubles."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE_SHA = 'd0037219f34fdc54b85ca696095415209531d5350652dea5ec86e508f75d5207'
FUNCTION_SHA = '430e9c02fd975fed0b0686659f9343761fe163b92c3ff42059300a8888401e31'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--php', default='php')
    parser.add_argument('--source', type=Path)
    args = parser.parse_args()
    reference = json.loads((ROOT / 'fixtures/source-gif-cleanup.json').read_text())
    assert reference['source_sha256'] == SOURCE_SHA
    assert reference['function_sha256'] == FUNCTION_SHA
    assert hashlib.sha256(reference['function'].encode()).hexdigest() == FUNCTION_SHA
    if args.source:
        raw = (args.source / reference['source']).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == SOURCE_SHA
        assert reference['function'] in raw.decode().replace('\r\n', '\n')
    # Namespace-local doubles intercept both shell calls. No legacy bootstrap,
    # real shell, upload bytes or external cleanup binary executes here.
    prelude = '''namespace GifReference;
$case = json_decode(stream_get_contents(STDIN), true, 512, JSON_THROW_ON_ERROR);
$calls = [];
function shell_exec($command) {
    global $case, $calls;
    $calls[] = $command;
    return $case['report'];
}
function system($command) {
    global $case, $calls;
    $calls[] = $command;
    return $case['system_result'];
}
'''
    invoke = '''$result = strip_gif_extra_data('owned.gif', $case['reported_size']);
echo json_encode([$result, $calls], JSON_THROW_ON_ERROR);
'''
    for case in reference['cases']:
        result = subprocess.run(
            [args.php, '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0',
             '-d', 'memory_limit=16M', '-d', 'disable_functions=exec,passthru,popen,proc_open',
             '-r', prelude + reference['function'] + invoke],
            input=json.dumps(case), text=True, capture_output=True, timeout=5, check=True)
        value, calls = json.loads(result.stdout)
        assert value == case['expected'], case['name']
        expected = ['/usr/local/bin/gifsicle --sinfo "owned.gif" 2>&1']
        if case['stripped']:
            expected.append('/usr/local/bin/gifsicle --no-comments --no-extensions "owned.gif" -o "owned.gif" >/dev/null 2>&1')
        assert calls == expected, case['name']
    print(f"All {len(reference['cases'])} captured GIF cleanup decisions match the pinned PHP function.")


if __name__ == '__main__':
    main()
