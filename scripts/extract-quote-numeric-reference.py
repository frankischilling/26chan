"""Controlled post-wrap numeric quote cases for the pinned source callbacks.

Expectations and SQL identities are explicit, independent inputs. --php-oracle
executes only the existing hash-checked callbacks with bounded lookup stubs.
"""
import argparse
import importlib.util
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / 'fixtures/source-quote-numeric.json'
spec = importlib.util.spec_from_file_location('quote_reference', Path(__file__).with_name('extract-quote-resolution-reference.py'))
ref = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ref)
# Explicit 64-bit PHP string/%d results, checked by the independent PHP oracle.
VALUES = [
    ('positive-sign', '+123', '123'),
    ('positive-zero-prefix', '+000123', '123'),
    ('negative', '-123', '-123'),
    ('negative-zero', '-0000', '0'),
    ('positive-zero', '+0', '0'),
    ('exponent', '12e1', '120'),
    ('positive-exponent', '12e+1', '120'),
    ('negative-exponent', '120e-1', '12'),
    ('fraction-truncated', '123e-1', '12'),
    ('fraction-to-zero', '1e-1', '0'),
    ('negative-fraction', '-123e-1', '-12'),
    ('negative-fraction-to-zero', '-1e-1', '0'),
    ('zero-exponent', '0e9', '0'),
    ('exponent-leading-zeroes', '0012e+001', '120'),
    ('double-rounding', '9007199254740993e0', '9007199254740992'),
    ('integer-overflow', '9223372036854775808', '9223372036854775807'),
    ('unsigned-overflow', '18446744073709551615', '9223372036854775807'),
    ('positive-overflow', '+99999999999999999999', '9223372036854775807'),
    ('negative-overflow', '-99999999999999999999', '-9223372036854775808'),
    ('finite-exponent-overflow', '1e19', '9223372036854775807'),
    ('infinite-exponent', '1e309', '0'),
    ('negative-infinite-exponent', '-1e309', '0'),
    ('underflow', '1e-9999', '0'),
]


def generate():
    baseline = ref.load_fixture()
    cases = []

    def add(name, spelling, lookup, resto=None, kind='cross_board', board='co', source='g', current='100', local=False):
        assert ref.canonical(lookup) or (int(lookup) <= 0 and resto is None)
        label = ('&gt;&gt;' if kind == 'same_board' else '&gt;&gt;&gt;/' + board + '/') + spelling
        if kind == 'cross_board' and board not in baseline['allowlist']:
            state, thread, href = 'plain', None, None
        elif resto is None or (source == 'mlp' and board in ('b', 'co')):
            state, thread, href = 'dead', None, None
        else:
            thread = lookup if resto == '0' else resto
            if local:
                state, href = 'local', '#p' + spelling
            else:
                state = 'thread'
                prefix = '/' if kind == 'same_board' else '//boards.example.test/'
                href = prefix + board + '/thread/' + (spelling if resto == '0' else resto) + '#p' + spelling
        html = label if state == 'plain' else '<span class="deadlink">' + label + '</span>' if state == 'dead' else '<a href="' + href + '" class="quotelink">' + label + '</a>'
        cases.append(dict(id=name, source_board=source, source_thread_id=current,
                          token_kind=kind, label_html=label, target_board=board,
                          target_post_id=spelling, lookup_post_id=lookup, lookup_resto=resto,
                          expected=dict(kind=state, thread_id=thread, href=href, html=html)))

    for name, spelling, lookup in VALUES:
        for resto in (('0', '1', None) if int(lookup) > 0 else (None,)):
            add(name + '-' + ('missing' if resto is None else 'op' if resto == '0' else 'reply'), spelling, lookup, resto)
    add('unknown-board-is-plain', '+123', '123', '0', board='unknown')
    add('mlp-forced-dead', '12e1', '120', '0', source='mlp')
    for spelling, local in [('9223372036854775808', True), ('18446744073709551615', False), ('99999999999999999999', False)]:
        add('local-overflow-' + spelling, spelling, str(ref.I64_MAX), '0', kind='same_board', board='g', current=str(ref.I64_MAX), local=local)
    add('local-overflow-rounded-current', '9223372036854775808', str(ref.I64_MAX), '0', kind='same_board', board='g', current=str(ref.I64_MAX - 1), local=True)
    return dict(schema_version=1, source_revision=ref.REVISION,
                reference='quote-resolution-reference.json',
                limits=['64-bit PHP string conversion at the source %d boundary; no database request is executed.',
                        'Controlled target visibility is not evidence for source database access policy.',
                        'Inputs represent the later post-wrap callback alphabet; earlier static links and wrapping are separate tests.'],
                cases=cases, lexical_cases=[])


def load_fixture():
    fixture = generate()
    ref.require(json.loads(FIXTURE.read_text()) == fixture, 'numeric fixture differs from explicit expectations')
    return fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--write', action='store_true')
    parser.add_argument('--php-oracle', action='store_true')
    args = parser.parse_args()
    fixture = generate()
    if args.write:
        FIXTURE.write_text(json.dumps(fixture, indent=2) + '\n')
    else:
        load_fixture()
    print(f'quote numeric reference: {len(fixture["cases"])} controlled cases')
    if args.php_oracle:
        baseline = ref.load_fixture()
        ref.php_oracle({**baseline, 'cases': fixture['cases'], 'lexical_cases': []})


if __name__ == '__main__':
    main()
