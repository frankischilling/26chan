"""Extract static navigation evidence; never execute source PHP or JavaScript.

Run with --source PATH --write to regenerate, or omit --write to verify.
Without --source, verify captured source hashes and generated Rust offline.
"""
import argparse
import hashlib
import html
from html.parser import HTMLParser
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/navigation-reference.json'
RUST = ROOT / 'apps/public/src/views/navigation_data.rs'
REVISION = '545b7812d1849f7958d914950c91fdbbe38f6b22'
SOURCE_HASHES = {
    'config/global_config.ini': '8bebeedec119b30559cba4fdccfef416653294cf62118d10288434be99a3034d',
    'config/boards/test.config.ini': '72f1ddef8a63944bfb40e5d2e368c8ffeb42bef4c18b02a04d3638186a98b683',
    'header.txt': '8401f90cc013eb4e332ca5d72f578a837d36848ad82aa7ce3b4e8785a10f1c5a',
    'header-ws.txt': 'e292483d7f76796884f43d266f64d4351f01207628f4f01af281e47a7712234e',
    'www.4chan/data/boards.php': '8673f6030e3027e41417fe0b2cee917a07af1726512f7f5bca5582b9778f725f',
    'js/core.js': 'a9ab67bea1f51fcdaaac1a879098a4d5888622008bf7f26f1fe18861ea52aab5',
    'catalog.php': '9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946',
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
}
CAPTURED = ('header.txt', 'header-ws.txt', 'www.4chan/data/boards.php')
EXCERPTS = {
    'mobile_destination': ('js/core.js', 912, 919, 'b3dc4225ee41f27afb855874344a1469640a02595bac2d31cd06f076419b75e1'),
    'test_header': ('config/boards/test.config.ini', 167, 167, 'ad89ea9f6796117fce7072d37eaffee8ec970b368e7037b053e4736bdc6c204a'),
    'global_header': ('config/global_config.ini', 315, 315, 'ad89ea9f6796117fce7072d37eaffee8ec970b368e7037b053e4736bdc6c204a'),
    'mobile_sort': ('js/core.js', 921, 955, '942b8accaed39d5d048854a3db930fe5f5eecc2f8d9af20247f3d0ec46c1d170'),
    'footer_clone': ('js/core.js', 956, 978, '6c67a746c7a95dc4d3dfd02fe48e074423d31642a9efe27e35bfcbeac1bf324c'),
    'catalog_links': ('catalog.php', 192, 193, '8cbeefd85f055012cb1eafdc4208037c685c5745e408edd4e1952853cceeb766'),
    'archive_links': ('imgboard.php', 3191, 3196, '57620a93a2062d92d611a4aa8a0006b11e82da6966117ecec0c91764c5d892fb'),
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def header_groups(text):
    # Stop at the utility links; nested nwsb spans must remain inside each group.
    body = text.split('<span class="boardList">', 1)[1].split('<span id="navtopright">', 1)[0]
    groups = []
    for group in re.findall(r'\[(.*?)\]', body, re.S):
        rows = []
        for href, title, slug in re.findall(r'<a href="([^"]+)" title="([^"]*)">([a-z0-9]+)</a>', group):
            require(re.fullmatch(r'(?:\/\/boards\.4chan\.org)?/' + slug + '/', href), 'header href mismatch')
            rows.append([slug, html.unescape(title)])
        groups.append(rows)
    require([len(group) for group in groups] == [27, 2, 4, 4, 40], 'header group sizes changed')
    return groups


class HeaderClasses(HTMLParser):
    """Track immediate-parent classes, matching Core's parentNode lookup."""
    def __init__(self):
        super().__init__()
        self.stack = []
        self.rows = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == 'a' and any('boardList' in a.get('class', '').split() for _, a in self.stack):
            match = re.fullmatch(r'(?:\/\/boards\.4chan\.org)?/([a-z0-9]+)/', attrs.get('href', ''))
            require(match is not None, 'invalid board anchor')
            classes = self.stack[-1][1].get('class', '').split()
            self.rows.append([match.group(1), 'nwsb' in classes])
        if tag not in {'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'param', 'source', 'track', 'wbr'}:
            self.stack.append((tag, attrs))

    def handle_endtag(self, tag):
        for i in range(len(self.stack) - 1, -1, -1):
            if self.stack[i][0] == tag:
                del self.stack[i:]
                break


def header_classes(text):
    parser = HeaderClasses()
    parser.feed(text)
    return parser.rows


def configured_header(excerpts):
    values = []
    for name in ('global_header', 'test_header'):
        match = re.fullmatch(r'NAV_TXT = /www/global/yotsuba/([a-z-]+\.txt)\n', excerpts[name]['text'])
        require(match is not None, 'invalid NAV_TXT assignment')
        values.append(match.group(1))
    require(values == ['header.txt', 'header.txt'], 'configured header changed')
    return values[0]


def directory_labels(text):
    # Match complete literal records, without loading PHP or importing its flags.
    records = re.findall(r'array\("domain"=>"boards","dir"=>"([a-z0-9]+)","name"=>"([^"\\]*)","highlight"=>[01],"nws"=>[01]\)', text)
    require(len(records) == 78, 'directory record count changed')
    return [list(row) for row in records]


def validate_rows(groups, directory):
    header = [row for group in groups for row in group]
    for name, rows in [('header', header), ('directory', directory)]:
        require(len({row[0] for row in rows}) == len(rows), name + ' duplicate slug')
        require(not {'test', 'qb', 'j'} & {row[0] for row in rows}, name + ' contains internal board')
    h, d = dict(header), dict(directory)
    require(set(h) - set(d) == {'qa'}, 'header-only boards changed')
    require(set(d) - set(h) == {'asp', 'trash'}, 'directory-only boards changed')
    require({s: [h[s], d[s]] for s in h.keys() & d.keys() if h[s] != d[s]} == {
        'p': ['Photo', 'Photography'], 'diy': ['Do It Yourself', 'Do-It-Yourself'],
    }, 'label differences changed')
    for slug, label in {'p': 'Photo', 'diy': 'Do It Yourself', 'lgbt': 'LGBT',
                        's4s': 'Shit 4chan Says', 'qa': 'Question & Answer', 'vp': 'Pokémon'}.items():
        require(h[slug] == label, 'decoded header label changed: ' + slug)


def fixture_from(captured, excerpts):
    require(set(captured) == set(CAPTURED), 'captured source paths changed')
    for path, text in captured.items():
        require(digest(text.encode()) == SOURCE_HASHES[path], 'source hash mismatch: ' + path)
    require(set(excerpts) == set(EXCERPTS), 'excerpt names changed')
    for name, (path, first, last, sha) in EXCERPTS.items():
        require(excerpts[name] == dict(file=path, first_line=first, last_line=last,
                                      sha256=sha, text=excerpts[name]['text']), 'excerpt metadata mismatch: ' + name)
        require(digest(excerpts[name]['text'].encode()) == sha, 'excerpt hash mismatch: ' + name)
    groups = header_groups(captured['header.txt'])
    require(header_groups(captured['header-ws.txt']) == groups, 'worksafe header contract differs')
    directory = directory_labels(captured['www.4chan/data/boards.php'])
    validate_rows(groups, directory)
    active = configured_header(excerpts)
    classes = {p: header_classes(captured[p]) for p in ('header.txt', 'header-ws.txt')}
    require(all([s for s, _ in rows] == [s for g in groups for s, _ in g] for rows in classes.values()),
            'header class membership differs')
    return dict(configured_header=active, header_parent_nws=classes,
                source_revision=REVISION, files=SOURCE_HASHES, captured_sources=captured,
                source_excerpts=excerpts, header_groups=groups, directory_labels=directory,
                header_only=['qa'], directory_only=['asp', 'trash'],
                label_differences={'p': ['Photo', 'Photography'], 'diy': ['Do It Yourself', 'Do-It-Yourself']})


def extract(source):
    texts = {}
    for path, sha in SOURCE_HASHES.items():
        raw = (source / path).read_bytes()
        require(digest(raw) == sha, 'source hash mismatch: ' + path)
        texts[path] = raw.decode('utf-8')
    excerpts = {}
    for name, (path, first, last, sha) in EXCERPTS.items():
        text = '\n'.join(texts[path].splitlines()[first - 1:last]) + '\n'
        excerpts[name] = dict(file=path, first_line=first, last_line=last, sha256=sha, text=text)
    return fixture_from({p: texts[p] for p in CAPTURED}, excerpts)


def rust_string(value):
    # JSON's \\uXXXX syntax is invalid Rust; encode controls as Rust Unicode escapes.
    return '"' + ''.join('\\"' if c == '"' else '\\\\' if c == '\\' else
                         '\\u{' + format(ord(c), 'x') + '}' if ord(c) < 32 or ord(c) == 127
                         else c for c in value) + '"'


def rust_table(fixture):
    lines = ['// Generated by scripts/extract-navigation-reference.py; do not edit.',
             '// Source revision: ' + REVISION, '', '#[rustfmt::skip]',
             'pub(super) const HEADER_GROUPS: &[&[(&str, &str)]] = &[']
    for group in fixture['header_groups']:
        lines.append('    &[')
        lines.extend('        (' + rust_string(s) + ', ' + rust_string(t) + '),' for s, t in group)
        lines.append('    ],')
    lines.extend(['];', '', '#[rustfmt::skip]', 'pub(super) const DIRECTORY_LABELS: &[(&str, &str)] = &['])
    lines.extend('    (' + rust_string(s) + ', ' + rust_string(t) + '),' for s, t in fixture['directory_labels'])
    lines.extend(['];', '', '#[rustfmt::skip]',
                  'pub(super) const HEADER_NWS_SLUGS: &[&str] = &['
                  + ', '.join(rust_string(slug) for slug, nws in
                              fixture['header_parent_nws'][fixture['configured_header']] if nws) + '];'])
    return '\n'.join(lines + [''])


def load_fixture(path=FIXTURE):
    fixture = json.loads(path.read_text(encoding='utf-8'))
    expected = fixture_from(fixture['captured_sources'], fixture['source_excerpts'])
    require(fixture == expected, 'fixture contract differs from captured source')
    return fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    require(not args.write or args.source is not None, '--write requires --source')
    fixture = extract(args.source) if args.source else load_fixture()
    serialized = json.dumps(fixture, ensure_ascii=False, indent=2) + '\n'
    rust = rust_table(fixture)
    if args.write:
        FIXTURE.write_text(serialized, encoding='utf-8', newline='\n')
        RUST.write_text(rust, encoding='utf-8', newline='\n')
    else:
        require(FIXTURE.read_bytes() == serialized.encode(), 'fixture is stale; regenerate from pinned source')
        require(RUST.read_bytes() == rust.encode(), 'Rust table is stale; regenerate from pinned source')
    print('Verified navigation: 77 header anchors in 5 groups; 78 directory records.')


if __name__ == '__main__':
    main()
