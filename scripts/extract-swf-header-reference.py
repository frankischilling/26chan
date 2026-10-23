#!/usr/bin/env python3
"""Compare owned SWF headers with PHP 8.3 getimagesize.

This records the pinned qualification runtime, not an unknown historical PHP
installation. Malformed geometry accepted by PHP is retained as a security
boundary case, not an instruction to reproduce unsigned wrapping.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import zlib

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()


def rect(values):
    count = max(1, max(abs(value).bit_length() + 1 for value in values))
    bits = f'{count:05b}' + ''.join(f'{value & ((1 << count) - 1):0{count}b}' for value in values)
    bits += '0' * (-len(bits) % 8)
    return bytes(int(bits[index:index + 8], 2) for index in range(0, len(bits), 8))


cases = []
for name, values, accepted in [
        ('normal', [0, 12800, 0, 9600], True),
        ('fraction', [0, 211, 0, 399], True),
        ('wide', [0, 200020, 0, 20], True),
        ('zero', [0, 0, 0, 0], False),
        ('negative_origin', [-400, 400, -200, 200], False),
        ('reversed', [100, 50, 0, 400], False)]:
    for signature in [b'FWS', b'CWS']:
        # Owned background, DebugID and binary-data tags provide enough bytes
        # for PHP's fixed-size header reads; no actions or external resources.
        body = rect(values) + struct.pack('<HH', 24 * 256, 1)
        body += struct.pack('<H', 9 * 64 + 3) + b'\x12\x34\x56'
        body += struct.pack('<H', 63 * 64 + 16) + bytes(range(16))
        body += struct.pack('<HIHI', 87 * 64 + 63, 134, 1, 0) + bytes(range(128))
        body += struct.pack('<HH', 64, 0)
        data = signature + bytes([10]) + struct.pack('<I', 8 + len(body))
        data += zlib.compress(body) if signature == b'CWS' else body
        result = subprocess.run([args.php, '-d', 'memory_limit=16M', '-d', 'max_execution_time=2', '-r',
            '$v=getimagesizefromstring(hex2bin(stream_get_contents(STDIN))); echo json_encode($v);'],
            input=data.hex().encode(), capture_output=True, check=True, timeout=5)
        assert not result.stderr, result.stderr.decode()
        metadata = json.loads(result.stdout)
        cases.append(dict(name=name, signature=signature.decode(), twips=values,
                          bytes_hex=data.hex(), sha256=hashlib.sha256(data).hexdigest(),
                          source_geometry_comparable=accepted, php=metadata))
fixture = dict(scope=__doc__, runtime='PHP 8.3 getimagesize', cases=cases)
encoded = json.dumps(fixture, indent=2) + '\n'
path = ROOT / 'fixtures/swf-header-reference.json'
if args.check:
    assert path.read_text() == encoded
else:
    path.write_text(encoded)
print(f'Checked {len(cases)} owned SWF header cases.')
