#!/usr/bin/env python3
"""Prepare or verify pinned Tegaki assets without fetching or compiling code."""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import struct


ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / 'fixtures/tegaki-assets-reference.json'
SOURCE_PINS = {
    'tegaki.min.js': (110619, 'daea182c52df0c032eadbecb4de8f91f634a61bf82aaf35dda077fab50e68744'),
    'tegaki.css': (16766, '0f29e747f90131104d1058559a6ec8b559f24aae6379fc310cb9fa26262f244e'),
}
FONT_PIN = (3128, '889ad3a092c684e62064431fe8f81ec948316a7875a3290525edb6bfc4af080b')
EXPORT = b'export { Tegaki };\n'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def verify(data, size, digest, label):
    require(len(data) == size, f'{label}: byte count differs')
    require(hashlib.sha256(data).hexdigest() == digest, f'{label}: SHA-256 differs')
    return data


def read_record(record):
    return verify((ROOT / record['path']).read_bytes(), record['bytes'],
                  record['sha256'], record['path'])


def prepare(reference=None):
    manifest = json.loads(FIXTURE.read_text())
    require(manifest['schema'] == 1 and manifest['version'] == '0.9.4',
            'Unreviewed Tegaki manifest version')
    require(manifest['upstream']['revision'] == 'b6697b164d6e9866adb39be4ef8c9db7102c8947',
            'Unreviewed upstream revision')
    require(set(manifest['source_files']) == set(SOURCE_PINS), 'Source input set differs')
    sources = {}
    for name, (size, digest) in SOURCE_PINS.items():
        row = manifest['source_files'][name]
        sources[name] = verify(read_record(row), size, digest, name)
        if reference is not None:
            require((reference / row['reference_path']).read_bytes() == sources[name],
                    f'{name}: reference checkout differs')

    notices = {Path(row['path']).name: read_record(row) for row in manifest['notices']}
    require(set(notices) == {'tegaki-MIT.txt', 'tegaki-UZIP-MIT.txt',
                            'tegaki-font-NOTICE.txt', 'tegaki-font-OFL-1.1.txt',
                            'tegaki-fontawesome5-NOTICE.txt', 'tegaki-fontawesome5-LICENSE.txt'},
            'Dependency notice set differs')
    require(b'Copyright (c) 2015 Maxime Youdine' in notices['tegaki-MIT.txt'],
            'Tegaki copyright missing')
    require(b'Copyright (c) 2018 Photopea' in notices['tegaki-UZIP-MIT.txt'],
            'UZIP copyright missing')
    require(b'Copyright (C) 2016 by Dave Gandy' in notices['tegaki-font-NOTICE.txt']
            and b'Copyright (C) 2012 by Daniel Bruce' in notices['tegaki-font-NOTICE.txt'],
            'Font copyright missing')
    require(b'SIL OPEN FONT LICENSE Version 1.1 - 26 February 2007' in notices['tegaki-font-OFL-1.1.txt'],
            'Font license missing')
    config = json.loads(read_record(manifest['font_config']))
    require(config['name'] == 'tegaki', 'Font name differs')
    require({glyph['src'] for glyph in config['glyphs']} == {'fontawesome', 'entypo', 'custom_icons'},
            'Font dependency set differs')
    require({glyph['search'][0] for glyph in config['glyphs'] if glyph['src'] == 'custom_icons'}
            == {'spray-can-solid', 'pen-fancy-solid'}, 'Custom icon set differs')
    for record in manifest['custom_icon_provenance']['icons']:
        read_record(record)

    script = sources['tegaki.min.js']
    require(script.startswith(b'/*! tegaki.js, MIT License */'), 'Tegaki header missing')
    require(b'/*! UZIP.js, \xc2\xa9 2018 Photopea, MIT License */' in script, 'UZIP header missing')
    require(b'Tegaki={VERSION:"0.9.4"' in script, 'Runtime version differs')
    require(not re.search(rb'\beval\s*\(|\bnew\s+Function\b|\bimport\s*\(|\bnew\s+Worker\b|\.cssText\b|window\.Tegaki\b', script),
            'Unreviewed runtime authority')

    css = sources['tegaki.css'].replace(b'\r\n', b'\n')
    verify(css, manifest['upstream_css']['bytes'], manifest['upstream_css']['sha256'],
           'Normalized CSS compared with upstream 0.9.4')
    urls = re.findall(rb"url\('([^']+)'\)", css)
    require(len(urls) == 1 and urls[0].startswith(b'data:application/octet-stream;base64,'),
            'Expected exactly one embedded WOFF URL')
    font = base64.b64decode(urls[0].split(b',', 1)[1], validate=True)
    verify(font, *FONT_PIN, 'Extracted WOFF')
    require(font[:4] == b'wOFF' and struct.unpack('>I', font[8:12])[0] == len(font),
            'Invalid WOFF header')
    css = css.replace(urls[0], b'./tegaki-icons.v1.woff')
    require(re.findall(rb"url\('([^']+)'\)", css) == [b'./tegaki-icons.v1.woff'],
            'Unexpected output CSS URL')
    require(b'data:' not in css and b'@import' not in css, 'Unreviewed CSS resource')

    for data in notices.values():
        require(b'*/' not in data, 'Notice cannot be embedded safely in a comment')
    script_notices = (b'\n/*!\nTegaki\n\n' + notices['tegaki-MIT.txt']
                      + b'\nUZIP.js\n\n' + notices['tegaki-UZIP-MIT.txt'] + b'*/\n')
    font_notices = (b'\n/*!\n' + notices['tegaki-font-NOTICE.txt'] + b'\n'
                    + notices['tegaki-font-OFL-1.1.txt'] + b'*/\n'
                    + b'\n/*!\n' + notices['tegaki-fontawesome5-NOTICE.txt'] + b'\n'
                    + notices['tegaki-fontawesome5-LICENSE.txt'] + b'*/\n')
    outputs = {
        'tegaki-0.9.4.v1.js': script + script_notices + EXPORT,
        'tegaki-0.9.4.v1.css': css + font_notices,
        'tegaki-icons.v1.woff': font,
    }
    require({Path(row['path']).name for row in manifest['outputs']} == set(outputs),
            'Output set differs')
    result = []
    for row in manifest['outputs']:
        path = ROOT / row['path']
        require(row['path'] == 'apps/public/static/tegaki/' + path.name,
                'Unexpected output directory')
        require(row['release_path'] == '/static/tegaki/' + path.name,
                'Unexpected release path')
        content = outputs[path.name]
        verify(content, row['bytes'], row['sha256'], row['path'])
        result.append((path, content))
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Verify without writing files')
    parser.add_argument('--reference', type=Path, help='Also compare an original-source checkout')
    args = parser.parse_args()
    for path, content in prepare(args.reference):
        if args.check:
            require(path.read_bytes() == content, f'{path.relative_to(ROOT)}: generated bytes differ')
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
    print('Verified Tegaki 0.9.4 source pins, dependency notices, module, CSS and WOFF.')


if __name__ == '__main__':
    main()
