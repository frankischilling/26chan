import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { encode, gateMetadata, LIMITS, unavailable } from '../../scripts/windows-visual-afd.mjs';
const metadata = (fields = [{ name: 'Process', type: 'win:Pointer', scalar: true }]) => ({
  provider: 'Microsoft-Windows-Winsock-AFD', events: [{ id: 1, version: 0, level: 4,
    unsupported: false, fields }],
});
test('no unverified profile can enable capture, including plausible numeric metadata', () => {
  assert.equal(gateMetadata(metadata()).capture, 'unavailable');
  assert.equal(gateMetadata(metadata()).reason, 'provider-schema-unverified');
});
test('unknown string/blob/version/shape fail closed but preserve bounded schema identifiers', () => {
  for (const type of ['win:UnicodeString', 'win:Binary', 'win:Future']) {
    const output = gateMetadata(metadata([{ name: 'Address', type, scalar: true }]));
    assert.equal(output.reason, 'provider-schema-rejected');
    assert.equal(output.metadata_fields[output.metadata[0].fields[0]].type, type);
  }
  const input = metadata(); input.events[0].version = 4;
  const output = gateMetadata(input);
  assert.equal(output.reason, 'provider-schema-rejected');
  assert.equal(output.metadata[0].version, 4);
});
test('metadata is not an event data or exception/message channel', () => {
  const input = metadata([{ name: 'secret/path:bearer token', type: 'secret/path',
    scalar: false, value: 'super-secret-value', message: 'private command line' }]);
  input.message = 'private user address';
  const output = encode(gateMetadata(input));
  for (const secret of ['secret/', 'super-secret', 'command line', 'user address']) assert.ok(!output.includes(secret));
  assert.equal(unavailable('private exception').reason, 'diagnostic-unavailable');
});
test('metadata and output bounds are enforced', () => {
  const input = metadata(); input.events = Array(257).fill(input.events[0]);
  assert.equal(gateMetadata(input).reason, 'metadata-limit');
  input.events.length = 256;
  const output = gateMetadata(input);
  assert.equal(output.metadata.length, 256);
  assert.equal(output.metadata_truncated, false);
  assert.ok(Buffer.byteLength(encode(output)) <= LIMITS.outputBytes);
  assert.ok(Buffer.byteLength(encode({ bad: 'x'.repeat(100000) })) <= LIMITS.outputBytes);
});
test('CI retains only summary JSON; wrapper preserves command and cleanup hook', () => {
  const script = readFileSync(new URL('../../scripts/windows-visual-afd.ps1', import.meta.url), 'utf8');
  const ci = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  assert.ok(!/logman (create|start|stop|delete)/.test(script));
  assert.match(script, /ReparsePoint/);
  assert.ok(!script.includes('Remove-Item'));
  assert.match(script, /npm run test:themes -- --shard "\$env:THEME_SHARD\/4" --output "test-results\/windows-themes-\$env:THEME_SHARD"\s+\$testExit = \$LASTEXITCODE/);
  assert.match(script, /exit \$testExit/);
  assert.match(ci, /Clean owned private Windows AFD manifest inspection\s+if: always\(\)/);
  assert.match(ci, /test-results\/windows-afd\/summary.json/);
  assert.ok(!ci.includes('*.etl'));
});

test('one theme shard retains only sanitized provider inventory regardless of test outcome', () => {
  const script = readFileSync(new URL('../../scripts/windows-visual-afd.ps1', import.meta.url), 'utf8');
  const ci = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  assert.match(script, /if \(\$testExit -ne 0 -or \(\$env:THEME_SHARD -eq '1' -and \$diagnostic\)\) \{/);
  assert.match(script, /\(Get-Item -LiteralPath \$resultPath\)\.Length -gt 65536/);
  assert.match(script, /\$destination = Join-Path \(Get-Location\) 'test-results\/windows-afd'/);
  assert.match(script, /WriteAllText\(\(Join-Path \$destination 'summary\.json'\), \$diagnostic\)/);
  const themeJob = ci.split('  visual-windows-themes:\n')[1];
  const steps = themeJob.split('      - name: ');
  const inventory = steps.filter(step => step.startsWith('Retain sanitized Windows AFD provider inventory\n'));
  assert.equal(inventory.length, 1);
  assert.match(inventory[0], /\n        if: always\(\) && matrix\.shard == 1\n/);
  assert.match(inventory[0], /\n        uses: actions\/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a /);
  assert.match(inventory[0], /\n          path: test-results\/windows-afd\/summary\.json\n/);
  assert.match(inventory[0], /\n          if-no-files-found: ignore\n/);
  assert.match(inventory[0], /\n          retention-days: 3\n/);
  assert.match(inventory[0], /\n          include-hidden-files: false\n/);
  assert.ok(!/netlogs|\.etl|\.xml|metadata\.private|windows-visual-resources|\*\*/i.test(inventory[0]));
  const failure = steps.find(step => step.startsWith('Retain synthetic Windows theme shard failure diagnostics\n'));
  assert.match(failure, /\n        if: failure\(\)\n/);
  assert.match(failure, /\n            test-results\/\*\*\/netlogs\/\*\.json\n/);
  assert.match(failure, /\n            test-results\/windows-afd\/summary\.json\n/);
});

test('partial manifest keeps valid descriptors when another template or level is unavailable', () => {
  const input = metadata();
  input.events.push({ id: 2, version: 0, level: null, fields: [], unsupported: true });
  input.issues = ['provider-level-unavailable', 'provider-template-unavailable', 'private exception'];
  const result = gateMetadata(input);
  assert.equal(result.reason, 'provider-level-unavailable');
  assert.equal(result.metadata_partial, true);
  assert.equal(result.metadata.length, 2);
  assert.equal(result.metadata[1].level, null);
  assert.ok(!encode(result).includes('private exception'));
});

test('compact field dictionary preserves all 256 bounded repeated templates', () => {
  const input = metadata();
  input.events = Array.from({ length: 256 }, (_, id) => ({ ...input.events[0], id,
    fields: Array.from({ length: 16 }, () => input.events[0].fields[0]) }));
  const result = gateMetadata(input);
  assert.equal(result.metadata.length, 256);
  assert.equal(result.metadata_fields.length, 1);
  assert.equal(result.metadata_counts.events_total, 256);
  assert.equal(result.metadata_counts.events_retained, 256);
  assert.equal(result.metadata_counts.fields_received, 4096);
  assert.equal(result.metadata_counts.fields_retained, 4096);
  assert.equal(result.metadata_counts.events_total_exact, true);
  assert.equal(result.metadata_truncated, false);
  assert.ok(Buffer.byteLength(encode(result)) < LIMITS.outputBytes);
  assert.equal(result.capture, 'unavailable');
});

test('manifest constants and references distinguish IPv6 from variable binary shapes without approving either', () => {
  const input = metadata([
    { name: 'Address', type: 'win:Binary', out_type: 'win:IPv6', scalar: false,
      attributes: ['name', 'inType', 'outType', 'length'], length: '16' },
    { name: 'Address', type: 'win:Binary', out_type: 'win:SocketAddress', scalar: false,
      attributes: ['name', 'inType', 'outType', 'length'], length: 'AddressLen' },
    { name: 'Payload', type: 'win:Binary', scalar: false,
      attributes: ['name', 'inType', 'count'], count: 'BufferLength' },
  ]);
  const result = gateMetadata(input);
  assert.equal(result.metadata_fields[0].length, '16');
  assert.equal(result.metadata_fields[1].length, 'AddressLen');
  assert.equal(result.metadata_fields[2].count, 'BufferLength');
  assert.deepEqual(result.metadata_fields[0].attributes, ['inType', 'length', 'name', 'outType']);
  assert.ok(result.metadata_fields[0].rejections.includes('field-type-unreviewed'));
  assert.ok(result.metadata_fields[2].rejections.includes('field-name-unreviewed'));
  assert.equal(result.rejection_counts['field-type-unreviewed'], 3);
  assert.equal(result.capture, 'unavailable');
  assert.equal(result.reason, 'provider-schema-rejected');
});

test('attribute inventory cannot expose arbitrary values, addresses, messages or process IDs', () => {
  const input = metadata([{ name: 'Address', type: 'win:Binary', scalar: false,
    attributes: ['length', 'count', 'secret/path', 'a'.repeat(65)],
    count: '192.0.2.1', length: 'C:\\private', value: 'PRIVATE_PAYLOAD',
    process_id: 123456789, map: 'PRIVATE_MAP', message: 'PRIVATE_MESSAGE' }]);
  const result = gateMetadata(input);
  const output = encode(result);
  for (const value of ['192.0.2.1', 'private', 'PRIVATE_', '123456789', 'secret/', 'a'.repeat(65)]) {
    assert.ok(!output.includes(value));
  }
  assert.equal(result.metadata_fields[0].count, 'unknown');
  assert.equal(result.metadata_fields[0].length, 'unknown');
  assert.ok(result.metadata_fields[0].rejections.includes('field-identifier-invalid'));
});

test('byte budget rolls back entire events and new dictionary entries with accurate counts', () => {
  const input = metadata();
  input.events = Array.from({ length: 256 }, (_, id) => ({ ...input.events[0], id,
    fields: Array.from({ length: 16 }, (_, f) => ({ name: `Field_${id}_${f}_${'x'.repeat(40)}`,
      type: 'win:UInt32', scalar: true, attributes: ['name', 'inType'] })) }));
  const result = gateMetadata(input);
  assert.equal(result.metadata_truncated, true);
  assert.ok(result.metadata.length > 0 && result.metadata.length < 256);
  assert.equal(result.metadata_counts.events_total, 256);
  assert.equal(result.metadata_counts.events_retained, result.metadata.length);
  assert.equal(result.metadata_counts.fields_received, 4096);
  assert.equal(result.metadata_counts.fields_retained, result.metadata.length * 16);
  assert.equal(result.metadata_counts.unique_fields_retained, result.metadata_fields.length);
  assert.equal(result.metadata_fields.length, result.metadata.length * 16);
  assert.ok(Buffer.byteLength(encode(result)) <= LIMITS.outputBytes);
  assert.equal(JSON.parse(encode(result)).metadata.length, result.metadata.length);
});

test('discovery omissions and exact/lower-bound counts stay distinct from output truncation', () => {
  const input = metadata();
  Object.assign(input, { events_total: 300, events_total_exact: false, issues: ['metadata-limit'] });
  const result = gateMetadata(input);
  assert.equal(result.metadata_counts.events_total, 300);
  assert.equal(result.metadata_counts.events_total_exact, false);
  assert.equal(result.metadata_counts.events_received, 1);
  assert.equal(result.metadata_counts.events_retained, 1);
  assert.equal(result.metadata_partial, true);
  assert.equal(result.metadata_truncated, true);
  assert.equal(result.reason, 'metadata-limit');
});

test('malformed event and field shapes fail closed without throwing or copying data', () => {
  const input = metadata([null, 'private string']);
  input.events.push(null, { id: 2, version: 0, level: 4, fields: Array(17).fill({}) });
  const result = gateMetadata(input);
  assert.equal(result.reason, 'provider-schema-rejected');
  assert.equal(result.rejection_counts['event-shape-invalid'], 2);
  assert.equal(result.metadata_partial, true);
  assert.equal(result.metadata_truncated, true);
  assert.ok(!encode(result).includes('private string'));
});

test('wrapper collects dimensions and bounded counts while remaining metadata-only', () => {
  const script = readFileSync(new URL('../../scripts/windows-visual-afd.ps1', import.meta.url), 'utf8');
  assert.match(script, /Get-WinEvent -ListProvider 'Microsoft-Windows-Winsock-AFD'/);
  assert.match(script, /\$node.GetAttribute\('count'\)/);
  assert.match(script, /\$node.GetAttribute\('length'\)/);
  assert.match(script, /events_total = \$eligible; events_total_exact = \$enumerationComplete/);
  assert.match(script, /\$scanned -gt 4096/);
  assert.match(script, /\$events.Count -ge 256/);
  assert.match(script, /\$fields.Count -ge 16/);
  assert.match(script, /\$attributes.Count -ge 16/);
  assert.match(script, /DtdProcessing\]::Prohibit/);
  assert.ok(!/Get-WinEvent\s+-(?:Path|Filter|LogName)|Start-NetEvent|netsh\s|pktmon\s/.test(script));
});

test('template limits remain explicit even when every received event descriptor fits', () => {
  const input = metadata();
  input.events[0].unsupported = true;
  input.events[0].rejections = ['template-field-limit', 'private failure detail'];
  const result = gateMetadata(input);
  assert.equal(result.metadata_partial, true);
  assert.equal(result.metadata_truncated, false);
  assert.deepEqual(result.metadata[0].rejections, ['template-unsupported', 'template-field-limit']);
  assert.equal(result.rejection_counts['template-field-limit'], 1);
  assert.ok(!encode(result).includes('private failure'));
});

test('dimensions and attribute names retain strict independent bounds', () => {
  for (const value of ['65536', '-1', '1.25', '0x10', 'x'.repeat(65)]) {
    const result = gateMetadata(metadata([{ name: 'Address', type: 'win:Binary', scalar: false,
      length: value, attributes: Array(17).fill('length') }]));
    assert.equal(result.metadata_fields[0].length, 'unknown');
    assert.equal(result.metadata_partial, true);
    assert.ok(result.metadata_fields[0].rejections.includes('field-attribute-limit'));
  }
});

const enrichedMetadata = () => {
  const input = metadata();
  input.provider_guid = '01234567-89AB-CDEF-0123-456789ABCDEF';
  Object.assign(input.events[0], { opcode: 255, task: 65535,
    channel_name: 'Microsoft-Windows-Winsock-AFD/Operational',
    keyword_values: ['0x8000000000000000', '0xFFFFFFFFFFFFFFFF', '0x0020000000000001'],
    keywords_complete: true });
  return input;
};
const descriptor = result => result.metadata_descriptors[result.metadata[0].descriptor];

test('public provider and event descriptors retain exact bounded values without inferring raw dimensions', () => {
  const result = gateMetadata(enrichedMetadata());
  assert.equal(result.inventory_schema, 2);
  assert.equal(result.provider_guid, '01234567-89ab-cdef-0123-456789abcdef');
  assert.equal(result.provider_guid_available, true);
  assert.equal(result.metadata_partial, false);
  assert.deepEqual(descriptor(result), {
    opcode: 255, task: 65535, channel_name: 'Microsoft-Windows-Winsock-AFD/Operational',
    keyword_values: ['0x8000000000000000', '0xffffffffffffffff', '0x0020000000000001'],
    available: { opcode: true, task: true, channel_name: true, keyword_values: true },
    channel_id: null, keyword_mask: null, raw_descriptor_complete: false,
  });
  assert.equal(result.capture, 'unavailable');
  assert.equal(result.complete, false);
  assert.deepEqual(result.events, []);
});

test('old or missing public metadata remains explicitly unavailable, never defaulted to zero', () => {
  const result = gateMetadata(metadata());
  assert.equal(result.provider_guid, null);
  assert.equal(result.provider_guid_available, false);
  assert.equal(result.metadata_partial, true);
  assert.equal(result.metadata_counts.events_total_exact, true);
  assert.deepEqual(descriptor(result), { opcode: null, task: null, channel_name: null,
    keyword_values: null,
    available: { opcode: false, task: false, channel_name: false, keyword_values: false },
    channel_id: null, keyword_mask: null, raw_descriptor_complete: false });
  for (const key of ['opcode', 'task', 'channel_name', 'keyword_values', 'keywords_complete']) {
    const input = enrichedMetadata(); delete input.events[0][key];
    assert.equal(gateMetadata(input).metadata_partial, true, key);
  }
});

test('descriptor numbers reject malformed, out-of-range and precision-losing values independently', () => {
  for (const key of ['opcode', 'task']) {
    const max = key === 'opcode' ? 255 : 65535;
    for (const value of [null, undefined, -1, max + 1, 1.5, NaN, Infinity,
      Number.MAX_SAFE_INTEGER + 1, '0', true, {}, []]) {
      const input = enrichedMetadata(); input.events[0][key] = value;
      const result = gateMetadata(input);
      assert.equal(descriptor(result)[key], null);
      assert.equal(descriptor(result).available[key], false);
      assert.equal(result.metadata_partial, true);
      assert.equal(descriptor(result)[key === 'opcode' ? 'task' : 'opcode'], key === 'opcode' ? 65535 : 255);
    }
    const input = enrichedMetadata(); input.events[0][key] = 0;
    assert.equal(descriptor(gateMetadata(input))[key], 0);
  }
});

test('keywords reject numbers, invalid widths and malformed collections without lossy conversion', () => {
  for (const values of [null, {}, '0x0000000000000001', [1], [9007199254740993],
    [-9223372036854775808], ['18446744073709551615'], ['-1'], ['0x1'],
    ['0x10000000000000000'], ['0x000000000000000g'], [null], ['0x0000000000000001\n'],
    ['0x0000000000000001', 'PRIVATE_KEYWORD_CANARY'], Array(65).fill('0x0000000000000001')]) {
    const input = enrichedMetadata(); input.events[0].keyword_values = values;
    const result = gateMetadata(input);
    assert.equal(descriptor(result).keyword_values, null);
    assert.equal(descriptor(result).available.keyword_values, false);
    assert.equal(result.metadata_partial, true);
    assert.ok(!encode(result).includes('PRIVATE_KEYWORD_CANARY'));
  }
  const input = enrichedMetadata();
  input.events[0].keyword_values = [];
  assert.deepEqual(descriptor(gateMetadata(input)).keyword_values, []);
  assert.equal(descriptor(gateMetadata(input)).available.keyword_values, true);
  input.events[0].keywords_complete = false;
  assert.equal(descriptor(gateMetadata(input)).available.keyword_values, false);
  input.events[0].keyword_values = ['0x8000000000000001'];
  assert.deepEqual(descriptor(gateMetadata(input)).keyword_values, ['0x8000000000000001']);
  assert.equal(gateMetadata(input).metadata_partial, true);
});

test('GUID and channel values are strictly bounded identifiers; unknown properties never leak', () => {
  for (const value of ['PRIVATE VALUE CANARY', 'C:\\PRIVATE_CANARY', '../PRIVATE_CANARY',
    'https://PRIVATE_CANARY', 'x'.repeat(129), 'Application\n', 'Application\r', 123, null, {}, []]) {
    const input = enrichedMetadata(); input.provider_guid = value; input.events[0].channel_name = value;
    const result = gateMetadata(input);
    assert.equal(result.provider_guid, null);
    assert.equal(descriptor(result).channel_name, null);
    assert.equal(result.metadata_partial, true);
    assert.ok(!encode(result).includes('PRIVATE_'));
  }
  const input = enrichedMetadata();
  input.provider = 'Microsoft-Windows-Winsock-AFD';
  input.provider_path = 'PRIVATE_PROVIDER_PATH';
  Object.assign(input.events[0], { channel_id: 123, keyword_mask: '0xffffffffffffffff',
    message: 'PRIVATE_EVENT_MESSAGE', description: 'PRIVATE_EVENT_DESCRIPTION',
    pointer: '0xffffface12345678', data: 'PRIVATE_EVENT_PAYLOAD',
    available: { channel_id: true, keyword_mask: true }, raw_descriptor_complete: true });
  const result = gateMetadata(input);
  assert.equal(descriptor(result).channel_id, null);
  assert.equal(descriptor(result).keyword_mask, null);
  assert.equal(descriptor(result).raw_descriptor_complete, false);
  assert.ok(!encode(result).includes('PRIVATE_'));
  assert.ok(!encode(result).includes('ffffface12345678'));
});

test('descriptor dictionary is deduplicated and rolled back within the unchanged JSON budget', () => {
  const input = enrichedMetadata();
  input.events = Array.from({ length: 256 }, (_, id) => ({ ...input.events[0], id }));
  const compact = gateMetadata(input);
  assert.equal(compact.metadata.length, 256);
  assert.equal(compact.metadata_descriptors.length, 1);
  for (const event of input.events) {
    event.task = event.id;
    event.keyword_values = Array.from({ length: 64 }, (_, bit) => `0x${(1n << BigInt(bit)).toString(16).padStart(16, '0')}`);
  }
  const result = gateMetadata(input);
  assert.equal(result.metadata_truncated, true);
  assert.ok(result.metadata.length > 0 && result.metadata.length < 256);
  assert.equal(result.metadata_descriptors.length, result.metadata.length);
  assert.equal(result.metadata_counts.events_retained, result.metadata.length);
  for (const event of result.metadata) assert.ok(result.metadata_descriptors[event.descriptor]);
  assert.ok(Buffer.byteLength(encode(result)) <= 65536);
  assert.equal(JSON.parse(encode(result)).metadata.length, result.metadata.length);
});

test('all nine error-only future candidates stay metadata-only even with complete public descriptors', () => {
  const input = enrichedMetadata();
  input.events = [6, 9, 10, 11, 12, 13, 14, 17, 40].map(id => ({ ...input.events[0], id,
    fields: [{ name: 'Process', type: 'win:Pointer', scalar: true },
      { name: 'Endpoint', type: 'win:Pointer', scalar: true },
      { name: 'Error', type: 'win:UInt32', scalar: true }] }));
  const result = gateMetadata(input);
  assert.equal(result.metadata.length, 9);
  assert.equal(result.capture, 'unavailable');
  assert.equal(result.reason, 'provider-schema-unverified');
  assert.equal(result.complete, false);
  assert.deepEqual(result.events, []);
  assert.deepEqual(result.counts, { scanned: 0, correlated: 0, failures: 0 });
  assert.equal(result.events_lost, null);
  assert.equal(result.buffers_lost, null);
  assert.equal(result.circular_overwrite, null);
});

test('wrapper uses only documented public properties and exact Int64 hex, with bounded keyword enumeration', () => {
  const script = readFileSync(new URL('../../scripts/windows-visual-afd.ps1', import.meta.url), 'utf8');
  assert.match(script, /\$provider.Id.ToString\('D'\)/);
  assert.match(script, /\$event.Opcode.Value/);
  assert.match(script, /\$event.Task.Value/);
  assert.match(script, /\$event.LogLink.LogName/);
  assert.match(script, /\$keywords = \$event.Keywords/);
  assert.match(script, /\$keywordValues.Count -ge 64/);
  assert.match(script, /\$keyword.Value -isnot \[long\]/);
  assert.match(script, /\$keyword.Value.ToString\('X16', \[Globalization.CultureInfo\]::InvariantCulture\)/);
  assert.ok(!/\[u?int(?:32)?\]\s*\$keyword|GetField|BindingFlags|ToXml\(|EventRecord|Start-Trace|Start-NetEvent|Set-Acl|icacls/i.test(script.replace('no EventRecord access', 'no record access')));
  assert.equal(LIMITS.metadataBytes, 262144);
  assert.equal(LIMITS.outputBytes, 65536);
});


test('GUID validation rejects trailing control characters rather than accepting a prefix', () => {
  for (const suffix of ['\n', '\r', '\r\n', '\0']) {
    const input = enrichedMetadata(); input.provider_guid += suffix;
    assert.equal(gateMetadata(input).provider_guid_available, false);
  }
});
