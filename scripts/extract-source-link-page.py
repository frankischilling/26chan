"""Verify the owned full-page source capture; optionally reproduce it from pinned files.

Only selected original rendering functions run, with controlled rows and stubs.
The captured page is test data, never an application route or executable script.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/source-link-page.json'
HTML_SHA256 = '615914bd0f92191881ffba72f15e5ea287cf1f6c06fd5513ff45a3071ad4e90e'
DARK_LINK_CSS = '.m-dark a, .m-dark div#absbot a { color: #81A2BE !important; }\n.m-dark a:hover { color: #5F89AC !important; }'
EXTENSION_SHA256 = '05b3b34f68377a44c071e4f74f629d2700fef61e064dcd2e836b161ee9ee0c31'
SOURCE_PINS = {'imgboard.php': {'sha256': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
                  'functions': {'renderPostHtml': {'first_line': 1915,
                                                   'last_line': 2430,
                                                   'sha256': 'e9e57dcc286f9f2e38fff610f48871bcea7afaf58d6fcd67d7bbfcbe42754e83'},
                                'head': {'first_line': 3199,
                                         'last_line': 3743,
                                         'sha256': '5127d49e0b532ae54ce4d20da29ce58d6a471197080f333de73e095070635298'},
                                'foot': {'first_line': 3755,
                                         'last_line': 3771,
                                         'sha256': '40d64a4742f8a0d036c507fcd94d2d963273034f02cbc151a723928efa5c54b8'},
                                'normalize_link_cb': {'first_line': 3839,
                                                      'last_line': 3875,
                                                      'sha256': '47cf833bd03ba53127bea504154cff704c41ad6def96bc3928d89a8449f35465'},
                                'normalize_links': {'first_line': 3877,
                                                    'last_line': 3883,
                                                    'sha256': 'abe28b76f608ee0492e19e62fcb15e744044ac37a39bc94053f21f1ac2513052'},
                                'post_resto': {'first_line': 3886,
                                               'last_line': 3903,
                                               'sha256': '017e44da0f390f44e375e5f9225e4fd1accf786d0a91c16dfeb0cfc676bf2461'},
                                'other_board_resto': {'first_line': 3940,
                                                      'last_line': 3963,
                                                      'sha256': '3a2b959d2948837d59811b7a069eb964e9ed0c070bbf63577f4a0170cb66ddab'},
                                'boards_matching_arr': {'first_line': 4013,
                                                        'last_line': 4021,
                                                        'sha256': 'a16f683ac58ada7b038ac20420bfbf4887b77269de9152cf5d96ee250376db94'},
                                'normalize_and_linkify': {'first_line': 4025,
                                                          'last_line': 4043,
                                                          'sha256': 'c97abfccf02997efdb20159e8d153fcb620bd6ffa8eb4608fc6d3117e73dbe34'},
                                'clean_internal_link': {'first_line': 4046,
                                                        'last_line': 4049,
                                                        'sha256': '836b17bbfe7038b7ef29b44ab617aeb8a85d992642e4f237c74f1e0165b3835d'},
                                'auto_link_static_cb': {'first_line': 4051,
                                                        'last_line': 4101,
                                                        'sha256': 'c3154e0dd264ccd9236b84d8128861c345ff89f9529c89c483c3c788145c559d'},
                                'auto_link': {'first_line': 4103,
                                              'last_line': 4120,
                                              'sha256': '6149c9e10e3db7c1adb6f15e4a4d6ebf4ef721de0f04584831e7d7c3d1e5f643'},
                                'auto_link_cb': {'first_line': 4122,
                                                 'last_line': 4139,
                                                 'sha256': 'c07581c7e2b2d3ad92c59cca03edcb3b8871b834a31eb4109d9e39258cbb2939'},
                                'parse_intraboard_link': {'first_line': 4141,
                                                          'last_line': 4170,
                                                          'sha256': '64c0460d098ac5adcd6b7877a4cb67dfac54310973c71fcf1b43d6f040c30f1f'},
                                'parse_interboard_link': {'first_line': 4172,
                                                          'last_line': 4216,
                                                          'sha256': 'e58a7f21b7d6c96009e05a07ef720ac3df384e5567ccae70c4999bfd56dda2da'}}},
 'views/imgboard.php': {'sha256': '6c2771682a20672f4f8395e78a2cb9fa80a9ed55933b77a0fcc54cc9f23e17eb',
                        'functions': {'form': {'first_line': 3,
                                               'last_line': 312,
                                               'sha256': '70a19de0f30da2690ae30248f2004e11da25725d7d08c7d3310809fc6a2e34b8'},
                                      'updatelog_real': {'first_line': 314,
                                                         'last_line': 727,
                                                         'sha256': 'f3c64d0279d25e0014f752c0be4e229733fbf6f42f5fa2f406b077473528f445'}}}}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def check(fixture):
    styles = json.loads((ROOT / 'fixtures/source-quote-styles.json').read_text())
    require(fixture['source_revision'] == styles['source_revision'], 'Source revision drift')
    require(fixture['sources'] == SOURCE_PINS, 'Rendering source pins drift')
    require(sha(fixture['html'].encode()) == HTML_SHA256, 'Original page capture drift')
    require(fixture['html_sha256'] == HTML_SHA256, 'Original page digest drift')
    require(set(fixture['themes']) == set(styles['themes']), 'Missing source theme')
    for theme, row in fixture['themes'].items():
        require(row['file'] == styles['themes'][theme]['file'], 'Theme path drift')
        require(sha(row['css'].encode()) == styles['themes'][theme]['source_sha256'], 'Full source stylesheet drift')
    require(sha(fixture['mobile_css'].encode()) == '3cb6eb4ec8fcb261b2f0496390484fa613d27ec1bad26fc97127b7dcb848d6ff', 'Mobile stylesheet drift')
    require(sha(fixture['mobile_nws_css'].encode()) == '872810133bc5dff715746b30127d531e7cd6d93d8405781af23dc7c4760eb023', 'NWS mobile stylesheet drift')
    require(fixture['dark_link_css'] == DARK_LINK_CSS, 'Dark comment-link rules drift')
    require(fixture['extension_sha256'] == EXTENSION_SHA256, 'Extension source pin drift')
    require(fixture['missing_source_assets'] == ['css/xa24extra.css'], 'Missing asset inventory drift')
    return fixture


def capture(source):
    extension = (source / 'js/extension.js').read_bytes()
    require(sha(extension) == EXTENSION_SHA256, 'Original extension hash differs')
    for rule in DARK_LINK_CSS.splitlines():
        require(rule in extension.decode(), 'Original dark comment-link rule differs')
    functions = {}
    for path, pin in SOURCE_PINS.items():
        raw = (source / path).read_bytes()
        require(sha(raw) == pin['sha256'], 'Original file hash differs: ' + path)
        lines = raw.decode().splitlines(keepends=True)
        for name, item in pin['functions'].items():
            code = ''.join(lines[item['first_line'] - 1:item['last_line']]).rstrip('\r\n')
            require(sha(code.encode()) == item['sha256'], 'Original function differs: ' + name)
            functions[name] = code
    baseline = json.loads((ROOT / 'fixtures/quote-resolution-reference.json').read_text())
    with tempfile.TemporaryDirectory(prefix='owned-source-link-page-') as directory:
        data = Path(directory) / 'input.json'
        data.write_text(json.dumps(dict(functions=functions, baseline=baseline)))
        result = subprocess.run(['php', '-n', str(ROOT / 'scripts/render-source-link-page.php'), str(data)],
                                capture_output=True, check=True, timeout=30)
        require(not result.stderr, 'Original page oracle wrote diagnostics: ' + result.stderr.decode())
    styles = json.loads((ROOT / 'fixtures/source-quote-styles.json').read_text())
    html = result.stdout.decode()
    return dict(source_revision=styles['source_revision'], sources=SOURCE_PINS,
                html_sha256=sha(result.stdout), html=html,
                themes={theme: dict(file=row['file'], css=(source / row['file']).read_bytes().decode())
                        for theme, row in styles['themes'].items()},
                mobile_css=(source / 'css/yotsubluemobile.css').read_bytes().decode(),
                mobile_nws_css=(source / 'css/yotsubamobile.css').read_bytes().decode(),
                dark_link_css=DARK_LINK_CSS, extension_sha256=EXTENSION_SHA256,
                missing_source_assets=['css/xa24extra.css'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    require(not args.write or args.source is not None, '--write needs the pinned original source')
    if args.source:
        fixture = check(capture(args.source))
        if args.write:
            FIXTURE.write_text(json.dumps(fixture, indent=2) + '\n')
        else:
            require(fixture == json.loads(FIXTURE.read_text()), 'Reproduced page differs from stored source capture')
    else:
        check(json.loads(FIXTURE.read_text()))
    print('Owned original full-page HTML and eight complete stylesheets match their pins.')


if __name__ == '__main__':
    main()
