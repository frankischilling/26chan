#!/usr/bin/env python3
"""Owned document fixtures through the explicit disposable PDF guest."""
import os
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest
from owned_process import run_owned

REPO = pathlib.Path(__file__).resolve().parents[2]


def document(streams, width=72, height=36, invalid_page=None):
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>']
    kids = ' '.join(f'{index * 2 + 3} 0 R' for index in range(len(streams)))
    objects.append(f'<< /Type /Pages /Count {len(streams)} /Kids [{kids}] >>'.encode())
    for index, stream in enumerate(streams):
        objects.append((f'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] '
                        '/Resources << /Font << /F1 << /Type /Font /Subtype /Type1 '
                        f'/BaseFont /Courier >> >> >> /Contents {index * 2 + 4} 0 R >>').encode())
        filter_name = ' /Filter /FlateDecode' if invalid_page == index else ''
        objects.append(f'<< /Length {len(stream)}{filter_name} >>\nstream\n'.encode()
                       + stream + b'\nendstream')
    output = bytearray(b'%PDF-1.4\n')
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(output))
        output.extend(f'{index} 0 obj\n'.encode() + obj + b'\nendobj\n')
    xref = len(output)
    output.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:
        output.extend(f'{offset:010} 00000 n \n'.encode())
    output.extend(f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    return bytes(output)


RED = b'1 0 0 rg 0 0 72 36 re f'
BLUE = b'0 0 1 rg 0 0 72 36 re f'


class PdfVmTest(unittest.TestCase):
    def decode(self, data, *, valid=True):
        with tempfile.TemporaryDirectory(prefix='owned-pdf-vm-') as directory:
            root = pathlib.Path(directory)
            source, output = root / 'input', root / 'output'
            source.write_bytes(data)
            result = run_owned([sys.executable, str(REPO / 'scripts/media/run-job.py'),
                                '--input-kind', 'pdf-v4', '--owned-diagnostics', os.environ['MEDIA_VM_TEST_CONFIG'],
                                str(source), str(output)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=35)
            jobs = pathlib.Path('/run/26chan-media-jobs')
            self.assertEqual(sorted(path.name for path in jobs.iterdir()), ['runner.lock'])
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            pixels = output.read_bytes()
            if not valid:
                # Transport success is not decoder approval. A rejected input
                # leaves the fixed disk zeroed for the host validator to reject.
                self.assertEqual(pixels, bytes(4_194_816))
                return None
            self.assertEqual(len(pixels), 4_194_816)
            self.assertEqual(pixels[:8], b'IBRGBA01', result.stderr.decode(errors='replace'))
            width, height = struct.unpack('>II', pixels[8:16])
            self.assertTrue(1 <= width <= 1024 and 1 <= height <= 1024)
            used = 16 + width * height * 4
            self.assertFalse(any(pixels[used:]))
            return pixels[:used]

    def test_compiled_owned_fixture_reports_renderer_readiness(self):
        with tempfile.TemporaryDirectory(prefix='owned-pdf-runtime-') as directory:
            root = pathlib.Path(directory)
            source, output = root / 'input', root / 'output'
            source.write_bytes(b'pdf-runtime')
            result = run_owned([sys.executable, str(REPO / 'scripts/media/run-job.py'),
                                '--input-kind', 'pdf-v4', '--owned-diagnostics', os.environ['MEDIA_VM_PROBE_CONFIG'],
                                str(source), str(output)], stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, timeout=35)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            data = output.read_bytes()
            self.assertEqual(data[:8], b'IBPDFDBG', result.stderr.decode(errors='replace'))
            length = int.from_bytes(data[8:12], 'big')
            self.assertLessEqual(length, 4096)
            report = data[12:12 + length].decode(errors='replace')
            for expected in ('uid=1000', 'tmp_write=true', 'version: success=true',
                             'nullpage: success=true', 'jpeg: success=true', 'decode: ok'):
                self.assertIn(expected, report, report)

    def test_first_page_font_and_recovery(self):
        first = self.decode(document([RED]))
        self.assertEqual(struct.unpack('>II', first[8:16]), (72, 36))
        self.assertEqual(first, self.decode(document([RED, BLUE])))
        blank = self.decode(document([b''], 144, 72))
        text = self.decode(document([b'BT /F1 18 Tf 8 24 Td (Owned text) Tj ET'], 144, 72))
        self.assertNotEqual(blank, text)
        for data in [b'%PDF-invalid', document([]), document([RED], 1025, 36),
                     document([RED, b'not a compressed stream'], invalid_page=1)]:
            self.decode(data, valid=False)
        self.assertEqual(first, self.decode(document([RED])))


if __name__ == '__main__':
    if os.geteuid() != 0 or 'MEDIA_VM_TEST_CONFIG' not in os.environ:
        raise SystemExit('requires an owned root VM qualification environment')
    unittest.main()
