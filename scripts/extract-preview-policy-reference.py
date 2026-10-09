#!/usr/bin/env python3
"""Extract pinned preview-policy evidence and independently derive bounded cases.

No PHP application, database, Rust implementation, or network is executed.
Use --source PATH --write to regenerate, --source PATH to check source bytes,
or no arguments to verify the captured excerpts and fixture offline.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/preview-policy-reference.json'
BOARD_FIXTURE = ROOT / 'fixtures/board-reference.json'
REVISION = '545b7812d1849f7958d914950c91fdbbe38f6b22'
EXCERPTS = {
    'global_limit': ('config/global_config.ini', 155, 155, 'b623e859e316eee56eebe5db6899330632bb47439c18af7205b62db1bde453a3'),
    'b_limit': ('config/boards/b.config.ini', 31, 31, 'bca5c9b747622c52186a27fe04c9b48b0b2e14df89f87eda1308cee9a8581569'),
    'bant_limit': ('config/boards/bant.config.ini', 57, 57, 'bca5c9b747622c52186a27fe04c9b48b0b2e14df89f87eda1308cee9a8581569'),
    't_limit': ('config/boards/t.config.ini', 19, 19, 'd9c8aa3afcd5cf456bfc13f36cebdc73347c5b56cdc0d713f08bc130843168cd'),
    'vg_limit': ('config/boards/vg.config.ini', 59, 59, '30e065a748cf78621cd7c540050bba684fa17ddd8c38a3ce47ef6a352b93dcfd'),
    'test_limit': ('config/boards/test.config.ini', 22, 22, 'b623e859e316eee56eebe5db6899330632bb47439c18af7205b62db1bde453a3'),
    'thread_counts_and_omissions': ('json.php', 13, 80, '550f709d3fb114305d1d361c511ac3307769fc58b45af4ab0e0598b3ed1261a5'),
    'thread_selection_and_catalog': ('json.php', 82, 104, 'ad5f8df3b72058e1f7bddf5ab7f195e5c59403b3be388919fa1670ccbe198fe0'),
    'full_thread_posts': ('json.php', 129, 157, '38b0c2ffa78f477e181aa53e9656d4c1d27d4c18e081dcdcc60eb7bb92d59259'),
    'index_policy': ('json.php', 551, 584, '676aa8a8b0d7830225db581983eb9b5b2360f46e144c248a522df7b28ffc829b'),
    'catalog_policy': ('json.php', 611, 636, 'f182fa6160156f02e8eb0fba62bb3bfe60fe6109633b9564103a574ffc386af5'),
    'html_default': ('views/imgboard.php', 427, 432, '52aa73f1cbeaa1ef3ad642e5f67cefca6d557fb0bd4e622a4c49eb74dd0166c7'),
    'html_selection': ('views/imgboard.php', 487, 531, '20f8297969257142a755905e7b169d536db1c4c6d3fd3caa13aefc932820c40c'),
    'cache_counts': ('imgboard.php', 950, 978, 'dcf62022cd778cb34923d9df41c54670b71f61a73c3473b7c12f9b2859ebe742'),
}


def require(value, message):
    if not value:
        raise ValueError(message)


def validate_captured(captured):
    require(set(captured) == set(EXCERPTS), 'captured excerpt names differ')
    for name, (_, _, _, digest) in EXCERPTS.items():
        require(hashlib.sha256(captured[name].encode()).hexdigest() == digest,
                f'captured source drift: {name}')


def capture(source):
    captured = {}
    for name, (file, first, last, digest) in EXCERPTS.items():
        raw = b''.join((source / file).read_bytes().splitlines(keepends=True)[first-1:last])
        require(hashlib.sha256(raw).hexdigest() == digest, f'source drift: {name}')
        captured[name] = raw.decode('utf-8')
    return captured


def config_value(text):
    match = re.fullmatch(r'REPLIES_SHOWN = ([0-5])\s*', text)
    require(match is not None, 'unexpected REPLIES_SHOWN declaration')
    return int(match[1])


def image(row):
    return row['fsize'] > 0 and not row['file_deleted']


def derive(configured, sticky, data):
    """Presentation branches transcribed from pinned PHP, not production calls.

    `replies` is the complete live cached child list supplied to the PHP renderer.
    Rows in excluded_deleted_replies were removed before that boundary. They are
    present only to document the store test setup, not to assert source RLS.
    """
    require(configured in (0, 1, 3, 5), 'unsupported independently qualified limit')
    require(isinstance(sticky, bool), 'sticky must be Boolean')
    rows = sorted(data['replies'], key=lambda row: int(row['id']))
    require(len({row['id'] for row in rows}) == len(rows), 'duplicate cached reply ID')
    effective = min(1, configured) if sticky else configured
    selected = rows[-effective:] if effective else []
    ids = [row['id'] for row in selected]
    reply_count = len(rows)
    image_count = sum(image(row) for row in rows)
    selected_images = sum(image(row) for row in selected)
    omitted_count = reply_count - len(selected)
    omitted_images = image_count - selected_images
    common = dict(replies=reply_count, images=image_count)
    preview = dict(common)
    if reply_count > effective:
        preview.update(omitted_posts=reply_count-effective, omitted_images=omitted_images)
    full = dict(common)
    if data['show_thread_uniques'] and data['unique_ips']:
        full['unique_ips'] = data['unique_ips']
    catalog = dict(op_extra=dict(preview))
    if reply_count > 0:
        catalog['last_reply_ids'] = ids
    return dict(effective_limit=effective, visible_reply_ids=ids,
                reply_count=reply_count, image_count=image_count,
                visible_image_count=selected_images, omitted_posts_count=omitted_count,
                omitted_images_count=omitted_images,
                index=dict(post_ids=[data['op_id']] + ids, op_extra=preview),
                catalog=catalog,
                full_thread=dict(post_ids=[data['op_id']] + [row['id'] for row in rows], op_extra=full))


def make_cases():
    cases = []
    for configured in (0, 1, 3, 5):
        for sticky in (False, True):
            for count in (0, 1, 2, 3, 4, 5, 6):
                for pattern in ('text', 'images', 'mixed-deletions'):
                    rows = []
                    for index in range(count):
                        rows.append(dict(id=str(101 + index),
                                         fsize=4096 if pattern == 'images' or (pattern == 'mixed-deletions' and index % 3 != 0) else 0,
                                         file_deleted=pattern == 'mixed-deletions' and index % 3 == 2))
                    # Reverse insertion order to exercise the source SORT_NATURAL step.
                    data = dict(op_id='100', op_fsize=9000, replies=list(reversed(rows)),
                                excluded_deleted_replies=[dict(id='107', fsize=8192, file_deleted=False),
                                                          dict(id='200', fsize=8192, file_deleted=False)] if pattern == 'mixed-deletions' else [],
                                show_thread_uniques=True, unique_ips=7)
                    cases.append(dict(id=f'limit-{configured}-sticky-{int(sticky)}-replies-{count}-{pattern}',
                                      configured_limit=configured, sticky=sticky, input=data,
                                      expected=derive(configured, sticky, data)))
    # Unique-IP field omission and zero-limit interaction are independent of counts.
    for enabled in (False, True):
        for uniques in (0, 4):
            data = dict(op_id='100', op_fsize=9000,
                        replies=[dict(id='101', fsize=4096, file_deleted=False)],
                        excluded_deleted_replies=[], show_thread_uniques=enabled, unique_ips=uniques)
            cases.append(dict(id=f'uniques-enabled-{int(enabled)}-count-{uniques}', configured_limit=0,
                              sticky=True, input=data, expected=derive(0, True, data)))
    return cases


def generate(captured):
    validate_captured(captured)
    return dict(schema_version=1, source_revision=REVISION,
                oracle_method='static-extraction-and-independent-source-derived-controlled-cache-expectations',
                source_execution='No original PHP application or extracted PHP was executed.',
                limits=['Input replies are complete controlled live-cache children. Excluded deleted replies are prefiltered scenario data; source database deletion/RLS semantics are not qualified.',
                        'Image counts include live replies with positive fsize and filedeleted=false. OP files and deleted reply files do not count.',
                        'post_ids and last_reply_ids project source posts and last_replies arrays to identities; unrelated post fields and last_modified are outside this fixture.',
                        'op_extra covers replies, images, omitted_posts, omitted_images, and unique_ips only. Semantic URLs, tail mode, and meta-board capcode summaries are outside scope.',
                        'SHOW_THREAD_UNIQUES and the resulting unique count are controlled inputs, not a new board-policy import.'],
                excerpts={name: dict(file=spec[0], start_line=spec[1], end_line=spec[2], sha256=spec[3]) for name, spec in EXCERPTS.items()},
                captured_sources=captured,
                board_policy=dict(stored_column='replies_shown', default=config_value(captured['global_limit']),
                                  explicit_overrides={board: config_value(captured[board + '_limit']) for board in ('b', 'bant', 't', 'vg', 'test')}),
                cases=make_cases())


def validate_board_import(fixture):
    boards = json.loads(BOARD_FIXTURE.read_text())['boards']
    default = fixture['board_policy']['default']
    overrides = fixture['board_policy']['explicit_overrides']
    for board in boards:
        expected = overrides.get(board['slug'], default)
        require(board.get('replies_shown') == expected, 'board replies_shown mismatch: ' + board['slug'])
        require(board['source_policy'].get('REPLIES_SHOWN') == str(expected), 'source policy mismatch: ' + board['slug'])


def load_fixture():
    fixture = json.loads(FIXTURE.read_text())
    require(fixture == generate(fixture['captured_sources']), 'preview fixture differs from independent derivation')
    validate_board_import(fixture)
    return fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path)
    parser.add_argument('--write', action='store_true')
    args = parser.parse_args()
    require(not args.write or args.source is not None, '--write requires --source')
    if args.source is not None:
        fixture = generate(capture(args.source))
        validate_board_import(fixture)
        if args.write:
            FIXTURE.write_text(json.dumps(fixture, indent=2) + '\n')
        else:
            require(fixture == load_fixture(), 'source and stored fixture differ')
    else:
        fixture = load_fixture()
    print(f'Preview policy: {len(fixture["cases"])} static source-derived cases, exact excerpts and board import verified; PHP not executed')


if __name__ == '__main__':
    main()
