"""Capture and qualify only pinned source page-title behavior.

Prepare without PHP:
  python3 scripts/extract-page-title-reference.py SOURCE FIXTURE --prepare
Qualify captured source in CI without a complete source checkout:
  python3 scripts/extract-page-title-reference.py --fixture-check FIXTURE --php php
Regenerate or check against a supplied source checkout by omitting --prepare.
The original application, includes, database and network are never executed.
"""
import argparse
import hashlib
import json
import resource
import subprocess
import tempfile
from pathlib import Path

SOURCE_HASHES = {
    'imgboard.php': 'caa787cde52eee4c52d85407b077f18938cd15923458a3d95c0c2c614ce7b445',
    'catalog.php': '9e41cd26755f9cee12e3fffa2050952a227e16362310b9b888438eba307af946',
}
# CRLF is normalized to LF before the exact, inclusive line excerpts are hashed.
EXCERPTS = {
    'generate_page_title': ('imgboard.php', 7990, 8029,
        'cd113985a2e5a63fe4939edf677686d1d64cf27f1aa69972730d73f4e5e6a321'),
    'page_title_composition': ('imgboard.php', 3628, 3659,
        '23b1452310103cf369d66bbc6cdbe8ec246cc1f949e3457c7f7baebad61784b3'),
    'visible_title_assignment': ('imgboard.php', 3427, 3427,
        'ddbdca4973761a392d2ba337f6c36b02fc48f291c4418192c5ca32753a2c8b8e'),
    'visible_title_heading': ('imgboard.php', 3728, 3728,
        '5ed8e0dc9ae910add0bbf704ca524c27e782411fda68213d8e32838c4697d613'),
    'catalog_title_assignment': ('catalog.php', 195, 195,
        'd77f7e4425db830dcc09100db11c3b8835a187753928ee3d399902cacf405578'),
    'catalog_title_element': ('catalog.php', 426, 426,
        '48e148512211ca6da7b7a0c74eb3382cd2fd12b077a728c6a4965cd336506824'),
}
MAX_OUTPUT = 131072
MAX_CASES = 128
MAX_INPUT = 8192


def html_escape(text):
    return (text.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')
            .replace('"', '&quot;').replace("'", '&#039;'))


def recipes():
    """Expected text is transcribed from the selected helper's operation order."""
    rows = []

    def case(name, expected, subject='', comment='', upload=False, sjis=False, board='g'):
        prefix = '[s4s] - ' if board == 's4s' else '/' + board + '/ - '
        rows.append(dict(id=name, subject=subject, comment=comment,
                         upload_board=upload, sjis=sjis, board=board,
                         source=dict(context=expected, fallback=expected == '',
                                     title=prefix + (html_escape(expected) if expected else 'No.420'))))

    case('empty_falls_back', '')
    case('subject_precedence', 'Preferred', subject='Preferred', comment='Ignored')
    case('subject_not_truncated', 'a' * 75, subject='a' * 75)
    case('subject_whitespace_wins', ' \t ', subject=' \t ', comment='Ignored')
    case('subject_entity_decode_once', '&lt;b&gt; & " \'', subject='&amp;lt;b&amp;gt; &amp; &quot; &#039;')
    case('subject_literal_markup_is_text', '<script>x</script>', subject='&lt;script&gt;x&lt;/script&gt;')
    case('subject_html401_entities', '&apos; &nbsp; &#65;', subject='&apos; &nbsp; &#65;')
    case('subject_unicode', '雪 😀 e\u0301', subject='雪 😀 e\u0301')
    case('internal_spoiler', 'Hidden', subject='SPOILER<>Hidden')
    case('empty_spoiler_uses_comment', 'Comment', subject='SPOILER<>', comment='Comment')
    case('literal_spoiler_is_preserved', 'SPOILER<>Literal', subject='SPOILER&lt;&gt;Literal')
    case('one_spoiler_marker_only', 'SPOILER<>Again', subject='SPOILER<>SPOILER&lt;&gt;Again')
    case('upload_numeric_prefix', 'Upload', subject='0012|Upload', upload=True)
    case('upload_then_spoiler', 'Hidden', subject='123|SPOILER<>Hidden', upload=True)
    case('upload_empty_prefix_result', 'Comment', subject='123|', comment='Comment', upload=True)
    case('numeric_prefix_outside_upload', '123|Title', subject='123|Title')
    case('upload_requires_ascii_digits', '１２|Title', subject='１２|Title', upload=True)
    case('upload_requires_leading_digits', '|Title', subject='|Title', upload=True)
    case('upload_requires_unsigned_prefix', '-2|Title', subject='-2|Title', upload=True)
    case('upload_prefix_once', '2|Title', subject='1|2|Title', upload=True)
    case('upload_order_before_spoiler', '12|Title', subject='SPOILER<>12|Title', upload=True)
    case('comment_exact_br', 'first second', comment='first<br>second')
    case('comment_other_break_tags', 'abcd', comment='a<br/>b<BR>c<br />d')
    case('comment_encoded_br_joins', 'firstsecond', comment='first&lt;br&gt;second')
    case('comment_keeps_whitespace', ' \tfirst  second\n third\r\n', comment=' \tfirst<br><br>second\n third\r\n')
    case('comment_strips_without_spaces', 'alphabetagamma', comment='alpha<s>beta</s>gamma')
    case('comment_html_only_falls_back', '', comment='<s></s><div></div>')
    case('comment_comments_only_fall_back', '', comment='<!-- hidden <b>x</b> -->')
    case('comment_script_body_is_text', 'alert(1)after', comment='<script>alert(1)</script>after')
    case('comment_quoted_tag_attributes', 'visible', comment='<a title="x>y" data-q=\'a>b\'>visible</a>')
    case('comment_encoded_quoted_attributes', 'visible', comment='&lt;a title=&quot;x&gt;y&quot;&gt;visible&lt;/a&gt;')
    case('comment_encoded_markup_removed', 'visible', comment='&lt;b&gt;visible&lt;/b&gt;')
    case('comment_decode_once', '&lt;b&gt;visible&lt;/b&gt;', comment='&amp;lt;b&amp;gt;visible&amp;lt;/b&amp;gt;')
    case('comment_html401_entities', '&apos; &nbsp; &#65; &#x41;', comment='&apos; &nbsp; &#65; &#x41;')
    case('comment_special_numeric_entities', '& " \' >', comment='&#38; &#x22; &#00039; &#62;')
    case('comment_special_named_entities', '& " \' >', comment='&amp; &quot; &#039; &gt;')
    case('comment_less_than_space', 'a < b > c', comment='a &lt; b &gt; c')
    case('comment_null_removed', 'ab', comment='a\u0000b')
    case('comment_trailing_unclosed_tag', 'before', comment='before<unfinished')
    case('comment_unclosed_quoted_tag', 'before', comment='before<a title="x>after')
    case('comment_nested_angle_brackets', 'abc', comment='a<<b>>b</b>c')
    case('comment_declaration', 'visible', comment='<!DOCTYPE html>visible')
    case('comment_declaration_quoted_gt', 'visible', comment='<!THING \"a>b\">visible')
    case('comment_comment_quoted_gt', 'beforeafter', comment='before<!-- \"a>b\" <b>hidden</b> -->after')
    case('comment_title_breakout', 'alert(1)visible', comment='&lt;/title&gt;&lt;script&gt;alert(1)&lt;/script&gt;visible')
    case('comment_php_unclosed_parenthesis', 'before', comment='before<?php ( ?>after')
    case('comment_php_quoted_terminator', 'beforeafter', comment='before<?php echo \"?>\"; ?>after')
    case('comment_processing_instruction', 'beforeafter', comment='before<?php echo "hidden";?>after')
    case('comment_xml_after_text', 'beforeafter', comment='before<?xml version="1.0">after')
    case('comment_50_ascii', 'x' * 50, comment='x' * 51)
    case('comment_50_unicode_scalars', '雪' * 49 + '😀', comment='雪' * 49 + '😀after')
    case('comment_combining_scalar_boundary', 'e\u0301' * 25, comment='e\u0301' * 26)
    case('comment_entity_count_after_decode', 'x' * 49 + '&', comment='x' * 49 + '&amp;after')
    case('comment_entity_literal_count', 'x' * 49 + '&', comment='x' * 49 + '&nbsp;after')
    case('sjis_replacement', 'before[SJIS]after', comment='before<span class="sjis">雪</span>after', sjis=True)
    case('sjis_disabled', 'before雪after', comment='before<span class="sjis">雪</span>after')
    case('sjis_empty_span', '[SJIS]', comment='<span class="sjis"></span>', sjis=True)
    case('sjis_multiple_spans', '[SJIS] [SJIS]', comment='<span class="sjis">one</span> <span class="sjis">two</span>', sjis=True)
    case('sjis_newline_does_not_match', 'a\nb', comment='<span class="sjis">a\nb</span>', sjis=True)
    case('sjis_br_matches_before_replacement', '[SJIS]', comment='<span class="sjis">a<br>b</span>', sjis=True)
    case('sjis_single_quote_does_not_match', 'text', comment="<span class='sjis'>text</span>", sjis=True)
    case('sjis_case_sensitive', 'text', comment='<span class="SJIS">text</span>', sjis=True)
    case('sjis_encoded_marker_does_not_match', 'text', comment='&lt;span class=&quot;sjis&quot;&gt;text&lt;/span&gt;', sjis=True)
    case('sjis_regex_accepts_suffix', '[SJIS]', comment='<span class="sjis"extra>text</span>', sjis=True)
    case('sjis_non_greedy_close', '[SJIS]tail', comment='<span class="sjis"><span>x</span>tail</span>', sjis=True)
    case('subject_wins_over_sjis', 'Subject', subject='Subject', comment='<span class="sjis">ignored</span>', sjis=True)
    case('literal_no_id_is_not_fallback', 'No.420', subject='No.420')
    case('s4s_bracket_prefix', 'Subject', subject='Subject', board='s4s')
    case('s4s_empty_fallback', '', board='s4s')
    # Subject output is already escaped in the original source, unlike comments.
    # Preserve it verbatim rather than re-escaping source-unsupported entities.
    for row in rows:
        subject = row['subject']
        if row['upload_board']:
            head, separator, tail = subject.partition('|')
            if separator and head and head.isascii() and head.isdigit():
                subject = tail
        if subject.startswith('SPOILER<>'):
            subject = subject[9:]
        if subject:
            prefix = '[s4s] - ' if row['board'] == 's4s' else '/' + row['board'] + '/ - '
            row['source']['title'] = prefix + subject
    return rows


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def capture(source):
    bodies = {}
    for file, expected in SOURCE_HASHES.items():
        path = source / file
        if path.stat().st_size > 1_000_000:
            raise ValueError('Source exceeds audited bound: ' + file)
        raw = path.read_bytes()
        if hashlib.sha256(raw).hexdigest() != expected:
            raise ValueError('Audited source changed: ' + file)
        bodies[file] = raw.decode('utf8').replace('\r\n', '\n').splitlines(keepends=True)
    snippets = {}
    for name, (file, start, end, expected) in EXCERPTS.items():
        text = ''.join(bodies[file][start - 1:end])
        if len(text) > 4096 or sha(text) != expected:
            raise ValueError('Audited source excerpt changed: ' + name)
        snippets[name] = dict(file=file, first_line=start, last_line=end, sha256=expected, text=text)
    return dict(
        reference='operator-supplied 4chan-old checkout',
        source_revision='545b7812d1849f7958d914950c91fdbbe38f6b22',
        files=SOURCE_HASHES,
        source_excerpts=snippets,
        expected_origin='Expected results transcribed from pinned source; this file does not claim a PHP execution. Run --fixture-check to qualify against the installed runtime.',
        scope='Thread subject/comment context plus captured title, visible heading and catalog title composition.',
        execution_boundary='Only the hash-pinned generate_page_title function executes. Board, upload and SJIS constants and two synthetic thread IDs are provided by the harness.',
        excludes=['original application and includes', 'database and network', 'actual board configuration',
                  'full HTML rendering', 'metatag generation', 'unbounded or non-UTF-8 historical data'],
        cases=recipes())


def validate(fixture):
    if fixture['files'] != SOURCE_HASHES or len(fixture['cases']) > MAX_CASES:
        raise ValueError('Fixture provenance or case bound changed.')
    for name, (file, start, end, expected) in EXCERPTS.items():
        snippet = fixture['source_excerpts'][name]
        if snippet != dict(file=file, first_line=start, last_line=end,
                           sha256=expected, text=snippet['text']) or sha(snippet['text']) != expected:
            raise ValueError('Captured source excerpt changed: ' + name)
    for row in fixture['cases']:
        if (not isinstance(row['subject'], str) or not isinstance(row['comment'], str)
                or not isinstance(row['upload_board'], bool) or not isinstance(row['sjis'], bool)
                or row['board'] not in ('g', 's4s')
                or len(json.dumps(row).encode()) > MAX_INPUT):
            raise ValueError('Case exceeds audited synthetic input: ' + str(row.get('id')))


def bound_child():
    resource.setrlimit(resource.RLIMIT_CPU, (3, 3))
    resource.setrlimit(resource.RLIMIT_FSIZE, (MAX_OUTPUT, MAX_OUTPUT))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))


def qualify(fixture, php):
    validate(fixture)
    selected = fixture['source_excerpts']['generate_page_title']['text']
    program = r'''
if (!extension_loaded('mbstring') || mb_internal_encoding() !== 'UTF-8') {
    throw new RuntimeException('UTF-8 mbstring is required for the title oracle.');
}
$r = json_decode(stream_get_contents(STDIN, 8193), true, 16, JSON_THROW_ON_ERROR);
define('JANITOR_BOARD', false);
define('UPLOAD_BOARD', $r['upload_board']);
define('SJIS_TAGS', $r['sjis']);
define('BOARD_DIR', $r['board']);
define('TITLE', 'Unused synthetic title');
''' + selected + r'''
$title = generate_page_title(420, $r['subject'], $r['comment']);
$second = generate_page_title(421, $r['subject'], $r['comment']);
$prefix = BOARD_DIR === 's4s' ? '[s4s] - ' : '/g/ - ';
if (strpos($title, $prefix) !== 0) throw new RuntimeException('Unexpected title prefix');
$fallback = $title !== $second;
$context = $fallback ? '' : htmlspecialchars_decode(substr($title, strlen($prefix)), ENT_QUOTES);
echo json_encode(['php' => PHP_VERSION, 'source' => [
    'context' => $context, 'fallback' => $fallback, 'title' => $title]], JSON_THROW_ON_ERROR);
'''
    if len(program) > 4096:
        raise ValueError('Harness exceeds audited bound.')
    version = None
    for row in fixture['cases']:
        # -n avoids startup files. Only the standard installed mbstring module
        # is enabled; no source application or fixture text is evaluated.
        with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
            run = subprocess.run([php, '-n', '-d', 'extension=mbstring', '-d', 'memory_limit=16M',
                            '-d', 'max_execution_time=3', '-d', 'default_charset=UTF-8',
                            '-r', program], input=json.dumps(row).encode(), stdout=stdout,
                           stderr=stderr, timeout=5, check=False, preexec_fn=bound_child)
            stdout.seek(0)
            output = stdout.read(MAX_OUTPUT + 1)
            stderr.seek(0)
            errors = stderr.read(4097)
        if run.returncode != 0 or errors or len(output) > MAX_OUTPUT:
            detail = (errors or output)[:1000].decode('utf8', 'replace')
            raise ValueError('PHP title oracle failed (UTF-8 mbstring is required): ' + detail)
        actual = json.loads(output)
        if version is not None and actual['php'] != version:
            raise ValueError('PHP runtime changed during qualification.')
        version = actual['php']
        if actual['source'] != row['source']:
            raise ValueError('Source title mismatch for ' + row['id'] + ': '
                             + json.dumps(dict(expected=row['source'], actual=actual['source'])))
    return version


def read_fixture(path):
    if path.stat().st_size > MAX_OUTPUT:
        raise ValueError('Fixture exceeds audited bound.')
    fixture = json.loads(path.read_text())
    validate(fixture)
    return fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path, nargs='?')
    parser.add_argument('output', type=Path, nargs='?')
    parser.add_argument('--php', default='php')
    parser.add_argument('--fixture-check', type=Path)
    parser.add_argument('--prepare', action='store_true', help='Capture source and transcribed expectations without running PHP.')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    if args.fixture_check:
        if args.source or args.output or args.prepare or args.check:
            parser.error('--fixture-check cannot be combined with source capture options.')
        fixture = read_fixture(args.fixture_check)
    else:
        if not args.source or not args.output:
            parser.error('Provide SOURCE OUTPUT or --fixture-check FIXTURE.')
        fixture = capture(args.source)
        validate(fixture)
        data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
        if len(data) > MAX_OUTPUT:
            raise ValueError('Fixture exceeds audited bound.')
        if args.check and args.output.read_bytes() != data:
            raise ValueError('Page-title source capture or expected cases differ.')
        if args.prepare:
            if not args.check:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.write_bytes(data)
            print(json.dumps(dict(cases=len(fixture['cases']), oracle='not executed', bytes=len(data))))
            return
    version = qualify(fixture, args.php)
    if not args.fixture_check and not args.check:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(data)
    print(json.dumps(dict(cases=len(fixture['cases']), oracle='passed', php=version)))


if __name__ == '__main__':
    main()
