"""Execute pinned, bounded adminopt parse/assignment/audit snippets, never the app.

Usage: LD_LIBRARY_PATH=... python3 scripts/extract-staff-grouped-options-reference.py \
  4chan-old apps/staff/tests/fixtures/staff-grouped-options.json --php /path/to/php
"""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--php', default='php')
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
hashes = {
    'admin.php': '4415d6684931efb93a9cc044bd69f770bd7bf1dc7b217e6f8cfffa0fe60418eb',
    'lib/auth.php': '98138062957155f859c6c2650613117290feeb386c6deb3a1097f5b62cbb6019',
}
sources = {}
for name, expected in hashes.items():
    raw = (args.source / name).read_bytes()
    if hashlib.sha256(raw).hexdigest() != expected:
        raise ValueError('Audited source changed: ' + name)
    sources[name] = raw.decode('utf8').replace('\r\n', '\n')
s, a = sources['admin.php'], sources['lib/auth.php']
start = s.index('function adminopt()')
x = s[start:s.index('function log_thread_opts_action(', start)]
parts = {
    'parse': x[x.index('$submit         ='):x.index('$post_id        =')],
    'manager': re.search(r"\$is_managerplus = has_level\( 'manager' \) \|\| has_flag\('developer'\);", x).group(),
    'preserve': re.search(r'if\( !\$is_managerplus \) \$post_permaage = \$permaage;', x).group(),
    'rank_reject': re.search(r'\$post_sticky == 1 && \( \$post_sticknum < 0 \|\| \$post_sticknum > 60 \)', x).group(),
    'rank_format': x[x.index('if( strlen( $post_sticknum )'):x.index('\n\t\t}', x.index('if( strlen( $post_sticknum )'))],
    'assignments': x[x.index('$vars = "";'):x.index('\n\t\tif( !$result = mysql_board_call( "UPDATE')],
    'rebuild_predicate': re.search(r'\$post_sticky != \$sticky \|\| \$post_closed != \$closed', x).group(),
    'audit': s[s.index('function log_thread_opts_action('):s.index('function get_board_options_html()')],
    'auth_helpers': a[a.index('function has_level('):a.index('function is_user()')],
    'level_map': re.search(r'\$levelorderf\s*=\s*(array\([\s\S]*?\));', a).group(1),
}
selected_hashes = {
    'parse': '3b0289c16e6bc6332956f42ce672d1afa99fc45eeea7845bd9054816ab7da0e6',
    'manager': '5e51d29d5248883879010bd6bd13497ff0d3f79fdbf22c4eac0091a1b626db6a',
    'preserve': 'e1a06ff7bb9775964b44a2f563272e19350293b30944b96e75e579fcb805f98f',
    'rank_reject': '85a6e0b6d4810f097baf92614fb8a1f3968552942fe6139f5ad9c3228e034d56',
    'rank_format': '765edd436ce0875fe1721ef9c6b799bbd9c12baee7dadb0dc2ad23e942edcd03',
    'assignments': 'ad3dae3bfc0c1c8336cabe5f72b1226e091de22f6e9538994d673c1edab2cd1a',
    'rebuild_predicate': 'fcc3cc2dc3a230d80bfff2526106ac3a596f7d9d84badefaec85a08c9b9386c4',
    'audit': '08e17fbdcbab63049e2ac503d23c704c52f00099174cfab461fe4471ec9ccf3a',
    'auth_helpers': '0a738830bffe5d4a6f037075724b468b455e0c89783a3e981a01c0d5c0e4e1b5',
    'level_map': '84248c53312c309109d8c20bb013f9818efccdf522b5702ed3ec23b2e01a0bc4',
}
for name, code in parts.items():
    if len(code) > 2048 or hashlib.sha256(code.encode()).hexdigest() != selected_hashes[name]:
        raise ValueError('Unexpected selected snippet: ' + name)

names = ('sticky', 'permasage', 'closed', 'permaage', 'undead')

def post(mask, rank='0'):
    return dict(submit='Set Options', sticknum=rank,
                **{name: str(int(bool(mask & (1 << bit)))) for bit, name in enumerate(names)})

roles = [
    ('mod', False, True, False), ('manager', False, True, False),
    ('admin', False, True, False), ('mod', True, True, False),
    ('mod', True, False, False), ('mod', True, True, True),
]
rows = []
runtime = None
for role, developer, allow_all, deny_noboard in roles:
    recipes = []
    if not developer and role in ('mod', 'manager'):
        recipes += [dict(group='canonical_mask_matrix', old_mask=old, requested_mask=new,
                         post=post(new)) for old in range(32) for new in range(32)]
    recipes += [dict(group='canonical_permission', old_mask=old, post=post(new))
                for old, new in [(0, 31), (31, 0), (8, 0), (0, 8)]]
    for rank in ('0', '59', '60', '-1', '61'):
        for sticky in (0, 1):
            recipes.append(dict(group='rank_boundary', old_mask=1, post=post(sticky, rank)))
    for old in (0, 31):
        recipes.append(dict(group='omitted_fields', old_mask=old, post={'submit': 'Set Options'}))
        recipes.append(dict(group='omitted_fields', old_mask=old,
                            post={'submit': 'Set Options', 'permasage': '1'}))
    for old in (0, 31):
        for omitted in (*names, 'sticknum'):
            data = post(31)
            del data[omitted]
            recipes.append(dict(group='omitted_single_field', omitted=omitted,
                                old_mask=old, post=data))
    recipes += [dict(group='rank_only', old_mask=1, old_rank=0, post=post(1, '59')),
                dict(group='unchanged', old_mask=31, old_rank=59, post=post(31, '59')),
                dict(group='missing_submit', old_mask=31, post={}),
                dict(group='empty_submit', old_mask=31, post={'submit': ''})]
    if role == 'manager':
        for field in (*names, 'sticknum'):
            for value in ('', '2', '-1', '01', '1.9', '1tail', 'true', ' 1 ', [], ['1']):
                data = post(1)
                data[field] = value
                recipes.append(dict(group='noncanonical_php_coercion', field=field,
                                    old_mask=0, post=data,
                                    rewrite_expectation='reject noncanonical scalar or array'))
        for raw in ('submit=Set+Options&sticky=0&sticky=1',
                    'submit=Set+Options&sticky=1&sticky=0',
                    'submit=Set+Options&sticky[]=1',
                    'submit=Set+Options&unexpected=1'):
            recipes.append(dict(group='request_hardening_difference', old_mask=0, raw_form=raw,
                                rewrite_expectation='reject duplicate, array, or unknown field'))
    context = dict(role=role, developer=developer, allow_all=allow_all, deny_noboard=deny_noboard)
    program = (
        "function is_local_auth(){return false;}define('BOARD_DIR','g');"
        "$_COOKIE['4chan_auser']='Owned source fixture';$_SERVER['REQUEST_TIME']=1800000000;"
        "function mysql_global_call($query,...$values){global $audit;"
        "$audit=['old_mask'=>$values[0],'new_mask'=>$values[1]];return true;}"
        "set_error_handler(function($number,$message){global $warnings;"
        "if(str_starts_with($message,'Undefined array key ')){$warnings++;return true;}return false;});"
        + '$auth=' + "json_decode('" + json.dumps(dict(level=role, guest=False,
                flags=['developer'] if developer else [], allow=['all'] if allow_all else ['g'],
                deny=['noboard'] if deny_noboard else [])) + "',true);"
        + '$levelorderf=' + parts['level_map'] + ';' + parts['auth_helpers'] + parts['audit']
        + "$names=['sticky','permasage','closed','permaage','undead'];"
        "$recipes=json_decode(stream_get_contents(STDIN),true,32,JSON_THROW_ON_ERROR);$rows=[];"
        "foreach($recipes as $recipe){$warnings=0;$_POST=$recipe['post']??[];"
        "if(isset($recipe['raw_form']))parse_str($recipe['raw_form'],$_POST);"
        "$row=['no'=>42,'name'=>'Anonymous','sub'=>'','com'=>'Owned comment','filename'=>'','ext'=>''];"
        "foreach($names as $bit=>$name)$row[$name]=($recipe['old_mask']>>$bit)&1;extract($row);"
        + parts['parse'] + parts['manager'] + parts['preserve']
        + "$parsed=[];foreach($names as $name)$parsed[$name]=${'post_'.$name};"
        "$out=['permaage_allowed'=>$is_managerplus,'parsed_flags'=>$parsed,'parsed_rank'=>$post_sticknum,"
        "'missing_key_warnings'=>$warnings];"
        "if($submit==''){$out['status']='form_only';}"
        'elseif(' + parts['rank_reject'] + "){$out['status']='rank_rejected';}else{"
        + parts['rank_format'] + 'ob_start();' + parts['assignments'] + 'ob_end_clean();'
        + "$audit=null;$logged=log_thread_opts_action($row,$post_sticky,$post_permasage,$post_closed,$post_permaage,$post_undead);"
        "$effective=$recipe['old_mask'];foreach($names as $bit=>$name){"
        "if(preg_match('/(?:^|,)'.$name.'=([01])(?:,|$)/',$vars,$m))"
        "$effective=($effective&~(1<<$bit))|((int)$m[1]<<$bit);}"
        "$out+=['status'=>'prepared','assignment_text'=>$vars,'effective_assignment_mask'=>$effective,"
        "'logged'=>$logged,'audit'=>$audit,'rebuild_predicate'=>" + parts['rebuild_predicate'] + "];"
        "}$rows[]=$recipe+['source'=>$out];}"
        "echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows],JSON_THROW_ON_ERROR);"
    )
    if len(program) > 12000:
        raise ValueError('Harness exceeds audited bound.')
    run = subprocess.run([args.php, '-n', '-d', 'memory_limit=64M', '-d', 'max_execution_time=5', '-r', program],
                         input=json.dumps(recipes).encode(), capture_output=True, timeout=10, check=True)
    if run.stderr or len(run.stdout) > 2_000_000:
        raise ValueError('Unexpected source snippet output: ' + run.stderr.decode()[:500])
    result = json.loads(run.stdout)
    if len(result['cases']) != len(recipes) or runtime not in (None, result['php']):
        raise ValueError('Incomplete cases or inconsistent PHP runtime.')
    runtime = result['php']
    rows += [dict(context=context, **row) for row in result['cases']]

# Harness self-checks protect against extraction/recorder omissions, not DB parity.
for row in rows:
    out = row['source']
    if row['group'] == 'canonical_mask_matrix':
        desired = row['requested_mask']
        expected = desired if out['permaage_allowed'] else (desired & ~8) | (row['old_mask'] & 8)
        assert out['effective_assignment_mask'] == expected
        assert out['logged'] == (row['old_mask'] != expected)
        assert out['audit'] == (dict(old_mask=row['old_mask'], new_mask=expected) if out['logged'] else None)
    if row['group'] in ('rank_only', 'unchanged'):
        assert not out['logged'] and out['audit'] is None
    if row['group'] == 'rank_boundary':
        rejected = row['post']['sticky'] == '1' and row['post']['sticknum'] in ('-1', '61')
        assert (out['status'] == 'rank_rejected') == rejected
fixture = {
    'reference': 'operator-supplied 4chan-old checkout',
    'source_revision': '545b7812d1849f7958d914950c91fdbbe38f6b22',
    'files': hashes, 'selected_sha256': selected_hashes, 'extractor_php': runtime,
    'scope': 'admin.php:3520-3734 selected parse, permission preservation, rank predicate/format, assignment and audit snippets; lib/auth.php level map and permission helpers; each authorization context runs in a separate PHP process because has_level caches its staff rank',
    'not_executed': ['whole application or adminopt function', 'database SELECT/UPDATE/INSERT',
                     'network, authentication, CSRF or route middleware', 'HTML form branch',
                     'rebuild_thread or timestamp storage/conversion'],
    'boundary_stubs': ['synthetic post, actor, saved flags and request time',
                       'local authentication forced false',
                       'mysql_global_call records audit masks and returns true without SQL execution',
                       'rank rejection predicate records rejection instead of executing source die',
                       'missing-field warnings counted and suppressed; PHP intval still executes',
                       'effective_assignment_mask is a synthetic projection of recorded assignment text, not a database result',
                       'parse_str is used only for labeled raw-form hardening differences'],
    'rank_60_persistence': 'UNPROVEN: source accepts rank 60 and emits root=20270101000060. Whether the unknown original database accepts/stores seconds=60 is not established. No claim that stubbed SQL executes.',
    'interpretation': ['canonical_mask_matrix covers all 32x32 requested transitions for both manager and moderator, including effective permission-preserved masks',
                       'omitted fields become zero through intval(null), except protected permaage retained for non-manager/non-developer',
                       'rank-only and flag no-op requests prepare last_modified but do not produce actions_log rows',
                       'noncanonical_php_coercion and request_hardening_difference are legacy observations, not required acceptance by the strict rewrite',
                       'strict rewrite rejects duplicate/unknown/array/malformed scalar fields intentionally; canonical 0/1 requests and omission semantics are the parity domain',
                       'noncanonical sticky/permasage/closed values can make audit truth differ from assignments; PHP 8.4 observations do not establish every historical PHP runtime behavior'],
    'cases': rows,
}
data = (json.dumps(fixture, ensure_ascii=True, indent=2) + '\n').encode()
if len(data) > 4_000_000:
    raise ValueError('Fixture exceeds audited bound.')
if args.check:
    if args.output.read_bytes() != data:
        raise ValueError('Staff grouped options reference differs.')
else:
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(data)
print(json.dumps({'cases': len(rows), 'canonical_matrix_cases': 2048, 'php': runtime,
                  'bytes': len(data), 'rank_60_persistence': 'UNPROVEN'}))
