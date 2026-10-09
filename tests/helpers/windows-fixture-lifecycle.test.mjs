import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { MARKER, LINE_BYTES, RECORD_LIMIT, ARTIFACT_BYTES, validateCheckpoint, createFixtureLifecycleCollector } from '../../scripts/windows-fixture-lifecycle.mjs';

const zero = { accepted: 0, active: 0, peak_active: 0, completed: 0, cancelled: 0, http_parse: 0, http_incomplete: 0, http_timeout: 0, http_other: 0, accept_errors: 0 };
const row = (boundary = 'startup', sequence = 1, counters = {}, extra = {}) => ({
  schema_version: 1, scope: 'fixture-listeners', boundary, sequence, counters: { ...zero, ...counters },
  overflow: false, output_failed: false, complete: boundary === 'shutdown', browser_lifetime: 'unavailable', ...extra,
});
const record = row => MARKER + JSON.stringify(row) + '\n';
const prefix = text => text.split(/(?<=\n)/).filter(Boolean).map(line => '[WebServer] ' + line).join('');
const push = (collector, row) => collector.push(prefix(record(row)));

test('fixed complete owned shutdown reconciles accepts, completions, errors and cancellations', () => {
  const collector = createFixtureLifecycleCollector();
  push(collector, row());
  push(collector, row('accepted', 2, { accepted: 1, active: 1, peak_active: 1 }));
  push(collector, row('accepted', 3, { accepted: 2, active: 2, peak_active: 2 }));
  push(collector, row('error', 4, { accepted: 2, active: 1, peak_active: 2, http_parse: 1 }));
  push(collector, row('ended', 5, { accepted: 2, peak_active: 2, http_parse: 1, cancelled: 1 }));
  push(collector, row('shutdown', 6, { accepted: 2, peak_active: 2, http_parse: 1, cancelled: 1 }));
  const result = collector.finish();
  assert.equal(result.status, 'complete');
  assert.equal(result.browser_lifetime, 'unavailable');
  assert.equal(result.checkpoints.length, 6);
});

test('every callback split and exact ANSI prefix preserves the original fixture line', () => {
  const raw = record(row());
  for (let split = 1; split < raw.length; split++) {
    for (const decoration of ['[WebServer] ', '\x1b[2m[WebServer] \x1b[22m']) {
      const collector = createFixtureLifecycleCollector();
      collector.push(decoration + raw.slice(0, split));
      collector.push(decoration + raw.slice(split));
      assert.equal(collector.finish().checkpoints.length, 1, `split ${split}`);
    }
  }
});

test('unrelated stdout and test-owned lookalikes never become fixture records', () => {
  const collector = createFixtureLifecycleCollector();
  collector.push(record(row()));
  collector.push(prefix(record(row())), {});
  collector.push(prefix(record(row())), undefined, {});
  collector.push('[OtherServer] ' + record(row()));
  collector.push(prefix('ordinary stdout contains ' + record(row())));
  assert.equal(collector.finish().status, 'unavailable');
});

test('missing shutdown, startup, callback data and partial records fail closed', () => {
  const none = createFixtureLifecycleCollector();
  assert.equal(none.finish().status, 'unavailable');
  const partial = createFixtureLifecycleCollector();
  push(partial, row());
  assert.equal(partial.finish().status, 'incomplete');
  const truncated = createFixtureLifecycleCollector();
  truncated.push(prefix(record(row()).slice(0, -1)));
  assert.equal(truncated.finish().status, 'invalid');
  const missingStart = createFixtureLifecycleCollector();
  push(missingStart, row('shutdown', 2));
  assert.equal(missingStart.finish().status, 'invalid');
  const missingChunk = createFixtureLifecycleCollector();
  missingChunk.push('[WebServer] ' + record(row()).slice(0, 80));
  missingChunk.push('unrelated output\n');
  missingChunk.push('[WebServer] ' + record(row()).slice(80));
  assert.equal(missingChunk.finish().status, 'invalid');
});

test('invalid schemas, duplicates, nonfinite/overflow counters and unsupported evidence reject', () => {
  for (const text of ['{', JSON.stringify(row()) + ' private', JSON.stringify(row()).replace('"schema_version":1', '"schema_version":1,"schema_version":1'), JSON.stringify(row()).replace('"accepted":0', '"accepted":-0')]) assert.throws(() => validateCheckpoint(text));
  for (const mutate of [
    value => value.raw_address = 'private', value => value.scope = 'browser', value => value.browser_lifetime = 'complete',
    value => value.counters.private = 1, value => delete value.counters.active,
    ...[-1, 0.5, 4294967296, Infinity, NaN, null, '1'].map(bad => value => value.counters.active = bad),
    value => value.complete = true, value => value.sequence = RECORD_LIMIT + 1,
  ]) {
    const value = row(); mutate(value);
    assert.throws(() => validateCheckpoint(JSON.stringify(value)));
  }
});

test('counters must reconcile and checkpoints cannot go backward or follow shutdown', () => {
  assert.throws(() => validateCheckpoint(JSON.stringify(row('accepted', 2, { accepted: 1 }))));
  assert.throws(() => validateCheckpoint(JSON.stringify(row('accepted', 2, { accepted: 3, active: 3, peak_active: 3 }))));
  for (const last of [row(), row('shutdown', 4), row('accepted', 3, { accepted: 1, active: 1, peak_active: 1 })]) {
    const collector = createFixtureLifecycleCollector();
    push(collector, row());
    push(collector, row('accepted', 2, { accepted: 2, active: 2, peak_active: 2 }));
    push(collector, last);
    assert.equal(collector.finish().status, 'invalid');
  }
  const ended = createFixtureLifecycleCollector();
  push(ended, row()); push(ended, row('shutdown', 2)); push(ended, row('shutdown', 3));
  assert.equal(ended.finish().status, 'invalid');
});

test('sparse checkpoints survive arbitrary unrelated output without claiming exact failure timing', () => {
  const collector = createFixtureLifecycleCollector();
  push(collector, row());
  for (let i = 0; i < 18; i++) collector.push(prefix('x'.repeat(512 * 1024) + '\n'));
  push(collector, row('accepted', 2, { accepted: 1024, active: 6, peak_active: 8, completed: 1018 }));
  const result = collector.finish();
  assert.equal(result.status, 'incomplete');
  assert.equal(result.checkpoints.at(-1).counters.accepted, 1024);
  assert.equal(result.browser_lifetime, 'unavailable');
});

test('oversized, malformed and missing output never exports raw text', () => {
  for (const body of [MARKER + 'SECRET'.repeat(LINE_BYTES) + '\n', MARKER + '{"secret":"PRIVATE"}\n']) {
    const collector = createFixtureLifecycleCollector(); collector.push(prefix(body));
    const result = collector.finish();
    assert.equal(result.status, 'invalid');
    assert.ok(!JSON.stringify(result).includes('SECRET'));
    assert.ok(!JSON.stringify(result).includes('PRIVATE'));
  }
  const invalid = createFixtureLifecycleCollector(); invalid.push(Buffer.from('private'));
  assert.equal(invalid.finish().status, 'invalid');
});

test('overflow and output loss stay incomplete; false terminal completeness rejects', () => {
  for (const flag of ['overflow', 'output_failed']) {
    const collector = createFixtureLifecycleCollector(); push(collector, row());
    push(collector, row('error', 2, {}, { [flag]: true }));
    push(collector, row('shutdown', 3, {}, { [flag]: true, complete: false }));
    assert.equal(collector.finish().status, 'incomplete');
    assert.throws(() => validateCheckpoint(JSON.stringify(row('shutdown', 2, {}, { [flag]: true }))));
  }
});

test('checkpoint and artifact budgets cover maximum valid counter widths', () => {
  const collector = createFixtureLifecycleCollector(); push(collector, row());
  let seq = 2;
  for (let exp = 0; exp < 32; exp++) {
    const value = 2 ** exp;
    push(collector, row('accepted', seq++, { accepted: value, active: value, peak_active: value }));
  }
  for (let exp = 0; exp < 32; exp++) {
    const value = 2 ** exp;
    push(collector, row('ended', seq++, { accepted: 2 ** 31, active: 2 ** 31 - value, peak_active: 2 ** 31, completed: value }));
  }
  for (let errors = 1; errors <= 16; errors++) push(collector, row('error', seq++, { accepted: 2 ** 31, peak_active: 2 ** 31, completed: 2 ** 31, accept_errors: errors }));
  push(collector, row('shutdown', seq++, { accepted: 2 ** 31, peak_active: 2 ** 31, completed: 2 ** 31, accept_errors: 16 }));
  const result = collector.finish();
  assert.equal(result.status, 'complete'); assert.equal(result.checkpoints.length, RECORD_LIMIT);
  assert.ok(Buffer.byteLength(JSON.stringify(result)) <= ARTIFACT_BYTES);
  result.checkpoints[0].scope = 'mutated';
  assert.equal(collector.finish().checkpoints[0].scope, 'fixture-listeners');
});

test('more than sixteen error checkpoints rejects without hiding the original counters', () => {
  const collector = createFixtureLifecycleCollector(); push(collector, row());
  for (let errors = 1; errors <= 17; errors++) push(collector, row('error', errors + 1, { accept_errors: errors }));
  const result = collector.finish();
  assert.equal(result.status, 'invalid');
  assert.equal(result.checkpoints.length, 17);
  assert.equal(result.checkpoints.at(-1).counters.accept_errors, 16);
});

test('CI keeps checkpoint artifacts separate and runs this validator suite', async () => {
  const workflow = await readFile(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  assert.match(workflow, /node --test tests\/helpers\/windows-fixture-lifecycle.test.mjs/);
  const step = workflow.split('      - name: ').find(step => step.startsWith('Retain fixture lifecycle checkpoints\n'));
  assert.match(step, /if: always\(\)/);
  assert.match(step, /path: test-results\/windows-themes-\*\/fixture-lifecycle.json\n/);
  assert.match(step, /retention-days: 3/);
  assert.ok(!step.includes('stdout'));
});

test('artifact writes are bounded, exclusive and reject linked output directories', async () => {
  const { mkdtemp, mkdir, rm, realpath, symlink, writeFile, readFile } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const path = await import('node:path');
  const { saveFixtureLifecycleEvidence } = await import('../../scripts/windows-fixture-lifecycle.mjs');
  const root = await realpath(await mkdtemp(path.join(tmpdir(), 'fixture-lifecycle-save-')));
  try {
    const output = path.join(root, 'windows-themes-1');
    const evidence = createFixtureLifecycleCollector().finish();
    await saveFixtureLifecycleEvidence(output, evidence);
    const saved = await readFile(path.join(output, 'fixture-lifecycle.json'), 'utf8');
    assert.deepEqual(JSON.parse(saved), evidence);
    await assert.rejects(saveFixtureLifecycleEvidence(output, evidence), { code: 'EEXIST' });
    const elsewhere = path.join(root, 'elsewhere'); await mkdir(elsewhere);
    const linked = path.join(root, 'windows-themes-2'); await symlink(elsewhere, linked, 'junction');
    await assert.rejects(saveFixtureLifecycleEvidence(linked, evidence), /Unsafe/);
    await assert.rejects(saveFixtureLifecycleEvidence(path.join(root, 'windows-themes-3'), 'x'.repeat(ARTIFACT_BYTES)), /Oversized/);
    await assert.rejects(saveFixtureLifecycleEvidence(path.join(root, 'wrong'), evidence), /Invalid/);
    const final = path.join(root, 'windows-themes-4'); await mkdir(final);
    await writeFile(path.join(final, 'fixture-lifecycle.json'), 'keep');
    await assert.rejects(saveFixtureLifecycleEvidence(final, evidence), { code: 'EEXIST' });
    assert.equal(await readFile(path.join(final, 'fixture-lifecycle.json'), 'utf8'), 'keep');
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('pinned Playwright webServer callback framing reaches the actual resource reporter without a browser', async () => {
  const { mkdtemp, writeFile, readFile, rm, realpath } = await import('node:fs/promises');
  const { tmpdir } = await import('node:os');
  const { execFile } = await import('node:child_process');
  const { promisify } = await import('node:util');
  const { createRequire } = await import('node:module');
  const path = await import('node:path');
  const require = createRequire(import.meta.url);
  assert.equal(require('playwright/package.json').version, '1.62.0');
  const root = await realpath(await mkdtemp(path.join(tmpdir(), 'fixture-lifecycle-playwright-')));
  try {
    const resultPath = path.join(root, 'result.json');
    const reporterURL = new URL('../../scripts/windows-visual-resource-reporter.mjs', import.meta.url).href;
    await writeFile(path.join(root, 'reporter.mjs'), `import Reporter from ${JSON.stringify(reporterURL)};\nimport { writeFileSync } from 'node:fs';\nexport default class extends Reporter { async onEnd() { writeFileSync(${JSON.stringify(resultPath)}, JSON.stringify(this.lifecycle.finish())); } }\n`);
    // Server startup emits a genuine checkpoint, but readiness uses stdout and
    // the test never requests a browser or listener. This exercises the pinned
    // prefixOutputLines and reporter dispatch, including forced ANSI prefixes.
    await writeFile(path.join(root, 'server.cjs'), `process.stdout.write(${JSON.stringify(record(row()).slice(0, 80))});\nsetImmediate(() => { process.stdout.write(${JSON.stringify(record(row()).slice(80))}); process.stdout.write('ready-owned-fixture\\n'); });\nsetInterval(() => {}, 1000);\n`);
    await writeFile(path.join(root, 'fixture.spec.cjs'), `const { test } = require(${JSON.stringify(require.resolve('@playwright/test'))}); test('owned framing', () => {});\n`);
    await writeFile(path.join(root, 'playwright.config.cjs'), `module.exports = { testDir: __dirname, testMatch: 'fixture.spec.cjs', workers: 1, reporter: [[${JSON.stringify(path.join(root, 'reporter.mjs'))}]], webServer: { command: '\"' + process.execPath + '\" server.cjs', cwd: __dirname, stdout: 'pipe', wait: { stdout: /ready-owned-fixture/ }, timeout: 10000 } };\n`);
    await promisify(execFile)(process.execPath, [require.resolve('@playwright/test/cli'), 'test', '--config', path.join(root, 'playwright.config.cjs')], {
      cwd: root, timeout: 20000, maxBuffer: 32768, env: { ...process.env, FORCE_COLOR: '1', WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS: '0', WINDOWS_THEME_STDERR_PROBE: '0' },
    });
    const evidence = JSON.parse(await readFile(resultPath, 'utf8'));
    assert.equal(evidence.status, 'incomplete');
    assert.deepEqual(evidence.checkpoints, [row()]);
  } finally { await rm(root, { recursive: true, force: true }); }
});
