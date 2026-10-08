// Discovery only: never start a browser or fixture server, and never filter tests.
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

export const WINDOWS_THEME_SHARDS = 8;
// Current measured maximum is 152. Growth above this ceiling requires reviewing
// the partition, not dropping cases or automatically increasing the budget.
export const MAX_WINDOWS_THEME_TESTS = 160;

export function discoveryIdentities(report) {
  assert.deepEqual(report.errors, [], 'Theme discovery must have no load errors');
  assert.equal(report.config.workers, 1, 'Theme discovery requires one worker');
  assert.equal(report.config.fullyParallel, false, 'Only the three reference matrices are shard-splittable');
  for (const project of report.config.projects) {
    // Playwright JSON exposes project retries, not per-suite overrides. The
    // current theme sources were reviewed to contain no retry overrides.
    assert.equal(project.retries, 0, 'Theme retries must remain disabled');
    assert.equal(project.repeatEach, 1, 'Each theme case must run once');
    assert.equal(project.timeout, 30_000, 'Theme test deadlines must remain unchanged');
  }
  const identities = [];
  const visit = (suite, parents = []) => {
    for (const spec of suite.specs ?? []) {
      for (const test of spec.tests) {
        assert.equal(test.expectedStatus, 'passed', 'Theme cases must not be skipped or expected to fail');
        // Discovery defers fail/slow effects until execution, but exposes their
        // static annotations. Do not admit weakened expectations or deadlines.
        assert.ok(!(test.annotations ?? []).some(annotation => ['skip', 'fixme', 'fail', 'slow'].includes(annotation.type)),
          'Theme cases must not carry skip, fixme, fail or slow annotations');
        assert.equal(test.timeout, 30_000, 'Individual theme deadlines must remain unchanged');
        assert.deepEqual(test.results, [], 'The shard guard must only discover tests');
        // Relative file + project + full title is stable across OS paths and
        // source-line shifts. Do not use machine-specific absolute locations.
        identities.push(JSON.stringify([
          spec.file.replaceAll('\\', '/'), test.projectName, [...parents, spec.title],
        ]));
      }
    }
    for (const child of suite.suites ?? []) visit(child, [...parents, child.title]);
  };
  visit(report);
  return identities;
}

export function verifyThemePartitions(all, shards, { count = WINDOWS_THEME_SHARDS, ceiling = MAX_WINDOWS_THEME_TESTS } = {}) {
  assert.ok(all.length > 0, 'Unsharded discovery must be nonempty');
  const expected = new Set(all);
  assert.equal(expected.size, all.length, 'Unsharded discovery contains duplicate identities');
  assert.equal(shards.length, count, 'Every configured shard must be discovered');
  const seen = new Set();
  for (const [index, shard] of shards.entries()) {
    assert.ok(shard.length > 0, `Theme shard ${index + 1} is empty`);
    assert.ok(shard.length <= ceiling, `Theme shard ${index + 1} has ${shard.length} tests, exceeding the reviewed ceiling ${ceiling}`);
    for (const identity of shard) {
      assert.ok(expected.has(identity), `Theme shard ${index + 1} contains an extra identity`);
      assert.ok(!seen.has(identity), `Theme shard ${index + 1} duplicates an identity`);
      seen.add(identity);
    }
  }
  assert.equal(seen.size, expected.size, `Theme shards are missing ${expected.size - seen.size} identities`);
  return {
    total: all.length, counts: shards.map(shard => shard.length), missing: 0, extra: 0, duplicates: 0,
    identitySha256: createHash('sha256').update(JSON.stringify([...expected].sort())).digest('hex'),
  };
}

export async function checkWindowsThemeShards() {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const cli = createRequire(import.meta.url).resolve('@playwright/test/cli');
  const execute = promisify(execFile);
  const env = { ...process.env };
  // Ensure the JSON reporter returns its manifest through stdout in CI too.
  for (const key of ['PLAYWRIGHT_JSON_OUTPUT_FILE', 'PLAYWRIGHT_JSON_OUTPUT_DIR', 'PLAYWRIGHT_JSON_OUTPUT_NAME']) delete env[key];
  const discover = async shard => {
    const args = [cli, 'test', '--config', 'playwright.themes.config.js', '--list', '--forbid-only', '--reporter=json'];
    if (shard) args.push('--shard', `${shard}/${WINDOWS_THEME_SHARDS}`);
    const { stdout } = await execute(process.execPath, args, {
      cwd: root, env, timeout: 60_000, maxBuffer: 8 * 1024 * 1024, windowsHide: true,
    });
    return discoveryIdentities(JSON.parse(stdout));
  };
  const all = await discover();
  const shards = [];
  for (let shard = 1; shard <= WINDOWS_THEME_SHARDS; shard++) shards.push(await discover(shard));
  return verifyThemePartitions(all, shards);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { console.log('Windows theme shard discovery:', JSON.stringify(await checkWindowsThemeShards())); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
