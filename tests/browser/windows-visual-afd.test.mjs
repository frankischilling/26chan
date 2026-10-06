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
