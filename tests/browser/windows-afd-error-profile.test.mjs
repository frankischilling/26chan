import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { AFD_PROVIDER_GUID, gateMetadata } from '../../scripts/windows-visual-afd.mjs';
import { ERROR_PROFILE_LIMITS, qualifySyntheticErrorRun, validateErrorProfile } from '../../scripts/windows-afd-error-profile.mjs';

const pointer = name => ({ name, type: 'win:Pointer', out_type: 'win:HexInt64' });
const uint = name => ({ name, type: 'win:UInt32', out_type: 'xs:unsignedInt' });
function inventory() {
  const events = [1, 6, 13, 40].map(id => ({ id, version: 0, level: 4, opcode: 0, task: 0,
    unsupported: false, rejections: [], channel_name: 'Microsoft-Windows-Winsock-AFD/Operational',
    keyword_values: ['0x8000000000000000'], keywords_complete: true,
    fields: [pointer('Process'), pointer('Endpoint'), ...(id === 1
      ? [uint('AddressFamily'), uint('SocketType'), uint('Protocol'), pointer('UserModePid')]
      : [{ name: 'Error', type: 'win:UInt32', out_type: 'win:NTStatus' }])].map(f =>
      ({ ...f, scalar: true, attributes: ['name', 'inType', 'outType'] })),
  }));
  return gateMetadata({ provider: 'Microsoft-Windows-Winsock-AFD', provider_guid: AFD_PROVIDER_GUID,
    events, events_total: 4, events_total_exact: true, issues: [],
    tdh: { status: 'ok', reason: null, api_status: null, provider_guid: AFD_PROVIDER_GUID,
      descriptors: events.map(e => ({ id: e.id, version: 0, level: 4, opcode: 0, task: 0,
        channel: 16, keyword_mask: '0x8000000000000000' })) } });
}
const json = () => JSON.stringify(inventory());
function record(id, width = 8, { process = 0xfedcba98n, endpoint = 0xdeadbeefn,
  pid = 0xabc123n, status = 0xc000009an } = {}) {
  const payload = Buffer.alloc(id === 1 ? 3 * width + 12 : 2 * width + 4);
  const put = (value, offset) => width === 4 ? payload.writeUInt32LE(Number(value), offset)
    : payload.writeBigUInt64LE(value, offset);
  put(process, 0); put(endpoint, width);
  if (id === 1) {
    payload.writeUInt32LE(2, 2 * width); payload.writeUInt32LE(1, 2 * width + 4);
    payload.writeUInt32LE(6, 2 * width + 8); put(pid, 2 * width + 12);
  } else payload.writeUInt32LE(Number(status), 2 * width);
  return { origin: 'synthetic', provider_guid: AFD_PROVIDER_GUID, id, version: 0,
    level: 4, channel: 16, task: 0, opcode: 0, keyword_mask: '0x8000000000000000',
    pointer_bytes: width, payload };
}
function closed(result) {
  assert.equal(result.capture, 'unavailable'); assert.equal(result.capture_ready, false);
  assert.equal(result.complete, false); assert.equal(result.live_layout_qualified, false);
  assert.equal(result.native_role_correlation, 'unverified');
}
function rejected(x) {
  const result = validateErrorProfile(JSON.stringify(x));
  assert.equal(result.metadata_profile_valid, false); assert.deepEqual(result.event_ids, []); closed(result);
}

test('exact candidate metadata is valid only for offline review', () => {
  const result = validateErrorProfile(json());
  assert.equal(result.metadata_profile_valid, true); assert.deepEqual(result.event_ids, [1, 6, 13, 40]); closed(result);
  assert.equal(inventory().reason, 'provider-schema-rejected'); // Legacy broad gate stays unchanged.
});
for (const [label, mutate] of [
  ['wrong provider', x => { x.provider_guid = '00000000-0000-0000-0000-000000000000'; }],
  ['partial metadata', x => { x.metadata_partial = true; }],
  ['truncated metadata', x => { x.metadata_truncated = true; }],
  ['unknown descriptor status', x => { x.raw_descriptor_status = 'unavailable'; }],
  ['missing raw completeness', x => { delete x.raw_descriptor_complete; }],
  ['runtime events', x => { x.events = [{}]; }],
  ['unknown version', x => { x.metadata[0].version = 1; }],
  ['duplicate identity', x => { x.metadata[1] = x.metadata[0]; }],
  ['missing event', x => { x.metadata.pop(); }],
  ['incorrect counts', x => { x.metadata_counts.fields_received++; }],
  ['bad reference', x => { x.metadata[0].fields[0] = -1; }],
  ['fractional reference', x => { x.metadata[0].descriptor = 0.1; }],
  ['field order', x => { x.metadata[0].fields.reverse(); }],
  ['extra field', x => { x.metadata[1].fields.push(x.metadata[1].fields[0]); }],
  ['numeric mask', x => { x.metadata_descriptors[0].keyword_mask = 0x8000000000000000; }],
  ['different mask', x => { x.metadata_descriptors[0].keyword_mask = '0x0000000000000000'; }],
  ['channel drift', x => { x.metadata_descriptors[0].channel_id = 17; }],
  ['opcode drift', x => { x.metadata_descriptors[0].opcode = 1; }],
  ['task drift', x => { x.metadata_descriptors[0].task = 1; }],
  ['level drift', x => { x.metadata[0].level = 3; }],
  ['pointer drift', x => { x.metadata_fields[0].type = 'win:UInt64'; }],
  ['array field', x => { x.metadata_fields[0].scalar = false; }],
  ['field count', x => { x.metadata_fields[0].count = '1'; }],
  ['field length', x => { x.metadata_fields[0].length = '8'; }],
  ['object attribute', x => { x.metadata_fields[0].attributes[0] = { toString: null }; }],
  ['duplicate attribute', x => { x.metadata_fields[0].attributes = ['name', 'name', 'inType']; }],
  ['unreviewed attribute', x => { x.metadata_fields[0].attributes.push('map'); }],
  ['NTSTATUS signed drift', x => { x.metadata_fields.find(f => f.name === 'Error').type = 'win:Int32'; }],
  ['NTSTATUS presentation drift', x => { x.metadata_fields.find(f => f.name === 'Error').out_type = 'win:Win32Error'; }],
]) test(`metadata rejects ${label}`, () => { const x = inventory(); mutate(x); rejected(x); });

test('metadata parsing is bounded and errors never echo input', () => {
  for (const input of [null, {}, '{secret', ' '.repeat(65537), 'null', '[]']) {
    const result = validateErrorProfile(input); closed(result);
    assert.equal(result.metadata_profile_valid, false); assert.ok(!JSON.stringify(result).includes('secret'));
  }
});
for (const width of [4, 8]) {
  test(`${width * 8}-bit synthetic layouts preserve high-bit NTSTATUS without exposing identities`, () => {
    const result = qualifySyntheticErrorRun(json(), [record(1, width), record(6, width),
      record(40, width, { status: 0xffffffffn }), record(13, width, { status: 0n })]);
    assert.equal(result.synthetic_qualified, true); closed(result);
    assert.equal(result.events[1].ntstatus, '0xc000009a');
    assert.equal(result.events[1].ntstatus_failure, true);
    assert.equal(result.events[2].ntstatus, '0xffffffff');
    assert.equal(result.events[3].ntstatus_failure, false);
    for (const e of result.events) assert.equal(e.socket_ordinal, 1);
    const text = JSON.stringify(result);
    for (const secret of ['fedcba98', 'deadbeef', 'abc123', '4275878552', '3735928559', '11256099',
      'payload', 'UserModePid', 'Process', 'Endpoint']) assert.ok(!text.includes(secret));
  });
  test(`${width * 8}-bit malformed lengths reject the whole run without partial exports`, () => {
    for (const id of [1, 6, 13, 40]) {
      const good = record(id, width);
      for (const length of [0, good.payload.length - 1, good.payload.length + 1, 65536]) {
        const bad = { ...good, payload: Buffer.alloc(length) };
        const result = qualifySyntheticErrorRun(json(), [record(1, width), bad]);
        assert.equal(result.synthetic_qualified, false); assert.deepEqual(result.events, []); closed(result);
      }
    }
  });
  test(`${width * 8}-bit reuse, closed and missing identities stay bounded candidates`, () => {
    const result = qualifySyntheticErrorRun(json(), [record(6, width), record(1, width), record(1, width),
      record(6, width), record(13, width), record(6, width), record(1, width), record(6, width, { process: 3n }),
      record(6, width, { endpoint: 4n })]);
    assert.deepEqual(result.events.map(e => e.socket_ordinal), [null, 1, 2, 2, 2, null, 3, null, null]);
    assert.equal(result.events[3].correlation, 'synthetic-candidate');
    assert.equal(qualifySyntheticErrorRun(json(), [record(6, width)]).events[0].socket_ordinal, null);
  });
}
test('64-bit synthetic identities retain all bits internally and do not alias 32-bit identities', () => {
  const result = qualifySyntheticErrorRun(json(), [record(1, 8, { process: 0x100000001n }),
    record(6, 8, { process: 1n }), record(1, 4, { process: 1n }), record(6, 8, { process: 0x100000001n })]);
  assert.deepEqual(result.events.map(e => e.socket_ordinal), [1, null, 2, 1]);
});
for (const [key, value] of [['origin', 'runtime'], ['provider_guid', 'other'], ['version', 1],
  ['id', 4], ['channel', 0], ['level', 3], ['task', 1], ['opcode', 1], ['keyword_mask', '0x0'],
  ['pointer_bytes', 16], ['payload', new Uint8Array(28)]]) {
  test(`synthetic records reject invalid ${key}`, () => {
    const result = qualifySyntheticErrorRun(json(), [{ ...record(6), [key]: value }]);
    assert.equal(result.synthetic_qualified, false); assert.deepEqual(result.events, []); closed(result);
  });
}
test('missing profile, empty input, null identities and width mismatches fail closed', () => {
  for (const records of [[], null, [null], [record(1, 8, { process: 0n })],
    [record(1, 8, { endpoint: 0n })], [record(1, 8, { pid: 0n })], [{ ...record(1, 8), pointer_bytes: 4 }]]) {
    assert.equal(qualifySyntheticErrorRun(json(), records).synthetic_qualified, false);
  }
  assert.equal(qualifySyntheticErrorRun('{}', [record(1)]).synthetic_qualified, false);
});
test('record and identity bounds discard partial results and expose truncation', () => {
  for (const records of [Array.from({ length: ERROR_PROFILE_LIMITS.records + 1 }, () => record(6)),
    Array.from({ length: ERROR_PROFILE_LIMITS.identities + 1 }, (_, i) => record(1, 8, { endpoint: BigInt(i + 1) }))]) {
    const result = qualifySyntheticErrorRun(json(), records);
    assert.equal(result.synthetic_qualified, false); assert.equal(result.records_truncated, true);
    assert.deepEqual(result.events, []); closed(result);
  }
  const max = qualifySyntheticErrorRun(json(), Array.from({ length: ERROR_PROFILE_LIMITS.records }, () => record(6)));
  assert.equal(max.synthetic_qualified, true);
  assert.ok(Buffer.byteLength(JSON.stringify(max)) <= ERROR_PROFILE_LIMITS.outputBytes);
});
test('NTSTATUS sign semantics are unsigned and remain separate from Winsock errors', () => {
  for (const status of [0, 0x7fffffff, 0x80000000, 0xc0000001, 0xffffffff, 10055]) {
    const event = qualifySyntheticErrorRun(json(), [record(6, 8, { status: BigInt(status) })]).events[0];
    assert.equal(event.ntstatus_failure, status >= 0x80000000);
    assert.equal(event.ntstatus, `0x${status.toString(16).padStart(8, '0')}`);
    assert.equal(event.winsock_error, undefined);
  }
});
test('offline module has no runtime entry point or capture-side integration', () => {
  const source = readFileSync(new URL('../../scripts/windows-afd-error-profile.mjs', import.meta.url), 'utf8');
  assert.ok(!/node:(?:child_process|fs)|process\.argv|EnableTraceEx2\s*\(|StartTrace\s*\(/.test(source));
  const collector = readFileSync(new URL('../../scripts/windows-visual-afd.ps1', import.meta.url), 'utf8');
  assert.ok(!collector.includes('windows-afd-error-profile'));
  const ci = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const profileLines = ci.split('\n').filter(line => line.includes('windows-afd-error-profile'));
  assert.deepEqual(profileLines.map(line => line.trim()), [
    'run: node --test tests/helpers/windows-fixture-lifecycle.test.mjs tests/helpers/visual-netlog.test.mjs tests/helpers/windows-theme-shards.test.mjs tests/helpers/windows-theme-stderr-filter.test.mjs tests/helpers/windows-theme-stderr-launcher.test.mjs tests/browser/windows-visual-afd.test.mjs tests/browser/windows-afd-error-profile.test.mjs',
  ]);
  assert.ok(!ci.includes('scripts/windows-afd-error-profile.mjs'));
  assert.ok(!/EnableTraceEx2\s*\(|StartTrace\s*\(|logman\s+(?:create|start)|\*\.etl/i.test(ci + collector));
});
