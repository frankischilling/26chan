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
    assert.equal(output.metadata[0].fields[0].type, type);
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
  assert.equal(output.metadata.length, 64);
  assert.equal(output.metadata_truncated, true);
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
