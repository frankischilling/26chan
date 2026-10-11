"""Pinned literal comment replacement from the supplied thread-move source.

Static expectations are hand-written. --php-oracle executes only the captured
str_replace assignment with synthetic strings, never the source application,
its configuration, database calls, filesystem operations or network requests.
"""
import argparse
import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/thread-transfer-reference.json'
REVISION = '545b7812d1849f7958d914950c91fdbbe38f6b22'
EXCERPTS = {
    'replace': (1555, 1555, 'c5f07b99defcdf7e7d49188e396ebc5fc9c84d133781b5ea892faedb352b555e'),
    'append_mapping': (1622, 1623, '4dfa5197be0f3f2272ca2185c4b017ef705bda59fb99f48eca9d7fd2b998b2f7'),
    'reply_selection': (1531, 1531, '41ba3265d8cd2dc22219f5ff72f090197d7873781a410ddd638cb35f90791fd2'),
    'final_presentation': (5828, 5843, '149504c3daac439759f1653ec9773901e4e475318bb5e3cb9be0e2a665f01469'),
    'posting_storage': (6525, 6529, '804d9b203051e5d5ddcc588cbe0eafed5470619920d7ea6eba3b648cee511235'),
    'transfer_storage': (1571, 1578, '0de2d3dd657790cd4c5203a3bbdaa6db4adcce469fedec3a25e5b5614cf54fae'),
    'late_render_call': (2032, 2032, '4354b3bf6ba8d9a7abc9093875a4b32d83857061db7c06a75764b8dc73c0884b'),
    'late_quote_resolution': (4103, 4120, '6f26c335781f59f6a73bfe7146f428530fbebf22cd051ea23cd937b93bc1c8e4'),
}


def require(value, message):
    if not value:
        raise ValueError(message)


def cases():
    result = []

    def add(name, source, pairs, expected):
        result.append(dict(id=name, input_html=source,
                           replacements=[dict(old=str(old), new=str(new)) for old, new in pairs],
                           expected_html=expected))

    add('op-has-no-prior-mappings', '&gt;&gt;1 &gt;&gt;2', [], '&gt;&gt;1 &gt;&gt;2')
    add('sequential-cascade-and-prefix', '&gt;&gt;1 &gt;&gt;2 &gt;&gt;12', [(1, 2), (2, 9)], '&gt;&gt;9 &gt;&gt;9 &gt;&gt;92')
    add('different-order', '&gt;&gt;1 &gt;&gt;2 &gt;&gt;12', [(2, 9), (1, 2)], '&gt;&gt;2 &gt;&gt;9 &gt;&gt;22')
    add('decimal-prefix-not-exact-token', '&gt;&gt;123 &gt;&gt;012 &gt;&gt;12', [(12, 7)], '&gt;&gt;73 &gt;&gt;012 &gt;&gt;7')
    add('one-pass-not-recursive', '&gt;&gt;1 &gt;&gt;11', [(1, 11)], '&gt;&gt;11 &gt;&gt;111')
    add('identity-pass', '&gt;&gt;1', [(1, 1)], '&gt;&gt;1')
    add('self-reference-unavailable', '&gt;&gt;2 &gt;&gt;1', [(1, 30)], '&gt;&gt;2 &gt;&gt;30')
    add('forward-reference-prefix-effect', '&gt;&gt;123', [(1, 8)], '&gt;&gt;823')
    add('word-break-stops-long-match', '&gt;&gt;1<wbr>2', [(12, 7)], '&gt;&gt;1<wbr>2')
    add('word-break-keeps-short-prefix', '&gt;&gt;1<wbr>2', [(1, 777)], '&gt;&gt;777<wbr>2')
    add('markup-stops-long-match', '&gt;&gt;1<b>2</b>', [(12, 7)], '&gt;&gt;1<b>2</b>')
    add('line-break-stops-long-match', '&gt;&gt;1<br>2', [(12, 7)], '&gt;&gt;1<br>2')
    add('entity-stops-long-match', '&gt;&gt;1&#50;', [(12, 7)], '&gt;&gt;1&#50;')
    add('entity-keeps-short-prefix', '&gt;&gt;1&#50;', [(1, 7)], '&gt;&gt;7&#50;')
    add('code-and-spoiler-remain-eligible', '<pre class="prettyprint">&gt;&gt;1</pre><s>&gt;&gt;1</s>', [(1, 4)], '<pre class="prettyprint">&gt;&gt;4</pre><s>&gt;&gt;4</s>')
    add('fixed-greentext-span', '<span class="quote">&gt;&gt;1 &gt; unchanged</span>', [(1, 7)], '<span class="quote">&gt;&gt;7 &gt; unchanged</span>')
    add('cross-board-is-not-same-board-prefix', '&gt;&gt;&gt;/g/12 &gt;&gt;12', [(12, 7)], '&gt;&gt;&gt;/g/12 &gt;&gt;7')
    add('double-escaped-user-entity-is-literal', '&amp;gt;&amp;gt;12 &gt;&gt;12', [(12, 7)], '&amp;gt;&amp;gt;12 &gt;&gt;7')
    add('existing-server-link-stays-frozen', '<a href="//boards.example.test/g/thread/12">12</a> &gt;&gt;12', [(12, 7)], '<a href="//boards.example.test/g/thread/12">12</a> &gt;&gt;7')
    add('unicode-and-punctuation', '猫 (&gt;&gt;12), &gt;&gt;12.', [(12, 7)], '猫 (&gt;&gt;7), &gt;&gt;7.')
    add('maximum-destination-id', '&gt;&gt;1', [(1, 9223372036854775807)], '&gt;&gt;9223372036854775807')
    add('expanded-lexical-value-is-not-clipped', '&gt;&gt;19', [(1, 9223372036854775807)], '&gt;&gt;92233720368547758079')
    return result


def validate(document):
    require(document.get('revision') == REVISION, 'source revision mismatch')
    require(set(document.get('captured', {})) == set(EXCERPTS), 'captured names mismatch')
    for name, (_, _, digest) in EXCERPTS.items():
        require(hashlib.sha256(document['captured'][name].encode()).hexdigest() == digest,
                f'captured source drift: {name}')
    require(document.get('cases') == cases(), 'static transfer cases changed')
    for case in document['cases']:
        for pair in case['replacements']:
            for value in pair.values():
                require(re.fullmatch(r'[1-9][0-9]*', value) is not None and int(value) <= 2**63-1,
                        'replacement ID outside supported positive range')
        # Independent Python builtin cross-check; expected values above are not generated.
        actual = case['input_html']
        for pair in case['replacements']:
            actual = actual.replace('&gt;&gt;' + pair['old'], '&gt;&gt;' + pair['new'])
        require(actual == case['expected_html'], f'static replacement mismatch: {case["id"]}')


def php_oracle(document):
    php = shutil.which('php')
    require(php is not None, 'PHP is required for --php-oracle')
    assignment = document['captured']['replace'].strip()
    require(assignment == "$comment = str_replace($from_pids, $to_pids, $post['com']);",
            'unexpected executable excerpt')
    program = '''$cases = json_decode(stream_get_contents(STDIN), true, 64, JSON_THROW_ON_ERROR);
$out = [];
foreach ($cases as $case) {
  $post = ['com' => $case['input_html']];
  $from_pids = []; $to_pids = [];
  foreach ($case['replacements'] as $pair) {
    $from_pids[] = '&gt;&gt;' . $pair['old'];
    $to_pids[] = '&gt;&gt;' . $pair['new'];
  }
''' + assignment + '''
  $out[] = $comment;
}
echo json_encode($out, JSON_THROW_ON_ERROR);
'''
    process = subprocess.run([php, '-n', '-d', 'allow_url_fopen=0', '-d', 'allow_url_include=0',
                              '-d', 'memory_limit=32M', '-r', program],
                             input=json.dumps(document['cases']).encode(),
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=10, check=True)
    require(json.loads(process.stdout) == [case['expected_html'] for case in document['cases']],
            'PHP literal replacement differs from static expectations')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--php-oracle', action='store_true')
    args = parser.parse_args()
    if args.write:
        require(args.source is not None, '--write requires --source')
        lines = (args.source / 'imgboard.php').read_bytes().splitlines(keepends=True)
        document = dict(revision=REVISION, captured={name: b''.join(lines[start-1:end]).decode()
                        for name, (start, end, _) in EXCERPTS.items()}, cases=cases())
        validate(document)
        FIXTURE.write_text(json.dumps(document, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    document = json.loads(FIXTURE.read_text(encoding='utf-8'))
    validate(document)
    if args.source:
        lines = (args.source / 'imgboard.php').read_bytes().splitlines(keepends=True)
        for name, (start, end, _) in EXCERPTS.items():
            require(b''.join(lines[start-1:end]).decode() == document['captured'][name], f'source drift: {name}')
    if args.php_oracle:
        php_oracle(document)
    print(f'{len(document["cases"])} pinned thread-transfer replacement cases passed')


if __name__ == '__main__':
    main()
