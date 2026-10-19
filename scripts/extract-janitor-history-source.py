#!/usr/bin/env python3
"""Pin the isolated history arithmetic without executing the source application."""
import argparse
import hashlib
import json
from pathlib import Path
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
raw = (args.source / 'reports/ReportQueue.php').read_bytes()
assert hashlib.sha256(raw).hexdigest() == 'e1eaa54bd229f0d718d026cb11a757576f67f02a86e21c4bf5ff051bbfbac0d1'
text = raw.decode().replace('\r\n', '\n')
method = text.split('  private function get_bans_summary(', 1)[1].split('    return $ret;', 1)[0]
fragment = method[method.index("    $now = $_SERVER['REQUEST_TIME'];"):].strip()
record = {'source':'reports/ReportQueue.php:get_bans_summary','sha256':hashlib.sha256(raw).hexdigest(),'php':fragment}
output = Path('fixtures/janitor-history-source.json')
encoded = json.dumps(record, indent=2) + '\n'
if args.check:
    assert output.read_text() == encoded
else:
    output.write_text(encoded)
