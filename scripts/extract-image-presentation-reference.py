#!/usr/bin/env python3
"""Capture source image geometry decisions without loading the source application.

The optional PHP check runs only pinned arithmetic and format/size conditions.
It performs no image decoding, filesystem writes, or external process execution.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SOURCE_SHA = 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445'
BLOCKS = {
    'thumbnail': ('\t// Resizing\n', '\t// the thumbnail is created',
                  'bab0966f371698608094d3ba919ef9e302d40e5637c64a4128d2372867233e19'),
    'mobile_condition': ('function resize_mobile_image(', '    $jpeg_quality = 80;',
                         '07d09493e64755a68748e7dbe46ef878201b035a9f1f32e61f5e456cd6bba4cc'),
    'mobile_dimensions': ('    $ratio = $w / $h;', '    $img_out = ImageCreateTrueColor',
                          '676f22403c6d01b86232d7fdf35bcbfaaad68d4dd7b90794c79688f52fef8281'),
}


def cases():
    # Literal expected values are independent of production image helpers.
    thumbnails = [
        ('no-upscale', [32, 20], [250, 250], [32, 20]),
        ('exact-op', [250, 250], [250, 250], [250, 250]),
        ('op-landscape', [640, 480], [250, 250], [250, 187]),
        ('reply-landscape', [640, 480], [125, 125], [125, 93]),
        ('vg-op', [640, 480], [200, 200], [200, 150]),
        ('portrait', [480, 640], [250, 250], [187, 250]),
        ('max-square', [10000, 10000], [250, 250], [250, 250]),
        ('different-bounds', [800, 600], [200, 100], [133, 100]),
        ('skinny-zero', [1, 10000], [250, 250], [0, 250]),
        ('wide-zero', [10000, 1], [125, 125], [125, 0]),
    ]
    mobile = [
        ('jpeg-at-side', '.jpg', [1024, 512], 200, None),
        ('jpeg-at-area', '.jpg', [2048, 256], 200, None),
        ('jpeg-over-area', '.jpg', [2048, 257], 200, [1024, 129]),
        ('png-at-bytes', '.png', [2048, 1024], 524288, None),
        ('png-over-bytes', '.png', [2048, 1024], 524289, [1024, 512]),
        ('portrait-round-half', '.jpg', [257, 2048], 200, [129, 1024]),
        ('mobile-square', '.jpg', [1025, 1025], 200, [1024, 1024]),
        ('unsupported-gif', '.gif', [2048, 1024], 524289, None),
        ('unsupported-extension', '.jpeg', [2048, 1024], 524289, None),
        ('thin-no-area', '.jpg', [10000, 1], 200, None),
        ('max-square', '.jpg', [10000, 10000], 200, [1024, 1024]),
    ]
    return {
        'thumbnail': [dict(id=name, dimensions=size, bounds=bounds, expected=expected)
                      for name, size, bounds, expected in thumbnails],
        'mobile': [dict(id=name, extension=ext, dimensions=size, bytes=length, expected=expected)
                   for name, ext, size, length, expected in mobile],
    }


def extract(source):
    raw = (source / 'imgboard.php').read_bytes()
    if hashlib.sha256(raw).hexdigest() != SOURCE_SHA:
        raise ValueError('Source image implementation differs from the reviewed snapshot.')
    text = raw.decode('utf-8').replace('\r\n', '\n')
    blocks = {}
    for name, (start, end, digest) in BLOCKS.items():
        begin = text.index(start)
        block = text[begin:text.index(end, begin)]
        if hashlib.sha256(block.encode()).hexdigest() != digest:
            raise ValueError('Source geometry block changed: ' + name)
        blocks[name] = block
    return {'source_sha256': SOURCE_SHA, 'blocks': blocks, 'cases': cases()}


def validate(reference):
    if reference.get('source_sha256') != SOURCE_SHA or reference.get('cases') != cases():
        raise ValueError('Image reference differs from the reviewed source or expectations.')
    blocks = reference.get('blocks', {})
    if set(blocks) != set(BLOCKS):
        raise ValueError('Image reference has unexpected source blocks.')
    for name, (_, _, digest) in BLOCKS.items():
        if hashlib.sha256(blocks[name].encode()).hexdigest() != digest:
            raise ValueError('Source geometry block changed: ' + name)


def check_php(reference, php):
    validate(reference)
    blocks = reference['blocks']
    # Complete the captured prefix with the captured geometry calculation and a
    # return, without including any decoder, image writer, or application code.
    program = 'function thumbnail($size, $width, $height) {\n' + blocks['thumbnail']
    program += 'return [$out_w, $out_h];\n}\n'
    program += blocks['mobile_condition'] + blocks['mobile_dimensions']
    program += 'return [$out_w, $out_h];\n}\nreturn null;\n}\n'
    program += '''$input = json_decode(stream_get_contents(STDIN), true, 512, JSON_THROW_ON_ERROR);
$result = ['thumbnail' => [], 'mobile' => []];
foreach ($input['thumbnail'] as $case) {
  $result['thumbnail'][] = thumbnail($case['dimensions'], $case['bounds'][0], $case['bounds'][1]);
}
foreach ($input['mobile'] as $case) {
  $value = resize_mobile_image(null, $case['dimensions'][0], $case['dimensions'][1], $case['bytes'], null, $case['extension']);
  $result['mobile'][] = $value === false ? null : $value;
}
echo json_encode($result, JSON_THROW_ON_ERROR);
'''
    completed = subprocess.run([php, '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0',
                                '-d', 'memory_limit=16M', '-r', program],
                               input=json.dumps(reference['cases']), text=True,
                               capture_output=True, timeout=10, check=True)
    actual = json.loads(completed.stdout)
    for group, rows in reference['cases'].items():
        expected = [row['expected'] for row in rows]
        if actual[group] != expected:
            raise AssertionError(f'{group} source oracle mismatch: {actual[group]!r}')
    print('All 21 source image geometry cases match the captured PHP decisions.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path, nargs='?')
    parser.add_argument('--output', type=Path, default=ROOT / 'fixtures/source-image-presentation.json')
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--php')
    parser.add_argument('--php-oracle', action='store_true')
    args = parser.parse_args()
    if args.source is None:
        if not args.php_oracle:
            parser.error('Supply the source checkout or --php-oracle.')
        reference = json.loads(args.output.read_text())
        check_php(reference, args.php or 'php')
        return
    reference = extract(args.source)
    data = json.dumps(reference, indent=2) + '\n'
    if args.check:
        if args.output.read_text() != data:
            raise SystemExit('Image presentation fixture differs from the reviewed source.')
    else:
        args.output.write_text(data)
    if args.php or args.php_oracle:
        check_php(reference, args.php or 'php')


if __name__ == '__main__':
    main()
