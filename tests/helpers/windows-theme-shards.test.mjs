import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { discoveryIdentities, verifyThemePartitions, WINDOWS_THEME_SHARDS, MAX_WINDOWS_THEME_TESTS } from '../../scripts/check-windows-theme-shards.mjs';

const manifest = () => ({
  errors: [], config: { workers: 1, fullyParallel: false, projects: [{ retries: 0, repeatEach: 1, timeout: 30_000 }] },
  suites: [{ title: 'matrix.spec.js', suites: [{ title: 'owned row', specs: [{
    title: 'paints the reference', file: 'nested\\matrix.spec.js',
    tests: [{ projectName: '', expectedStatus: 'passed', timeout: 30_000, results: [] }],
  }] }] }],
});

test('discovery preserves relative file, project and full title without OS paths or line positions', () => {
  const report = manifest();
  const ids = discoveryIdentities(report);
  assert.deepEqual(ids, [JSON.stringify(['nested/matrix.spec.js', '', ['matrix.spec.js', 'owned row', 'paints the reference']])]);
  report.suites[0].suites[0].specs[0].line = 999;
  assert.deepEqual(discoveryIdentities(report), ids);
});

test('load errors, concurrency, retries, repeats, skipped cases, execution and deadline changes fail closed', () => {
  for (const mutate of [
    report => report.errors.push({ message: 'load failure' }),
    report => report.config.workers = 2,
    report => report.config.fullyParallel = true,
    report => report.config.projects[0].retries = 1,
    report => report.config.projects[0].repeatEach = 2,
    report => report.config.projects[0].timeout = 60_000,
    report => report.suites[0].suites[0].specs[0].tests[0].expectedStatus = 'skipped',
    report => report.suites[0].suites[0].specs[0].tests[0].results.push({ status: 'passed' }),
    report => report.suites[0].suites[0].specs[0].tests[0].timeout = 60_000,
    ...['skip', 'fixme', 'fail', 'slow'].map(type => report => {
      report.suites[0].suites[0].specs[0].tests[0].annotations = [{ type }];
    }),
  ]) {
    const report = manifest(); mutate(report); assert.throws(() => discoveryIdentities(report));
  }
});

test('exact partition follows dynamically discovered cases and has an order-independent identity digest', () => {
  const options = { count: 2, ceiling: 3 };
  const result = verifyThemePartitions(['a', 'b', 'c'], [['b'], ['a', 'c']], options);
  assert.deepEqual({ ...result, identitySha256: undefined }, {
    total: 3, counts: [1, 2], missing: 0, extra: 0, duplicates: 0, identitySha256: undefined,
  });
  assert.equal(result.identitySha256, verifyThemePartitions(['c', 'b', 'a'], [['a', 'c'], ['b']], options).identitySha256);
  assert.equal(verifyThemePartitions(['a', 'b', 'c', 'new'], [['a', 'new'], ['b', 'c']], options).total, 4);
});

test('missing, extra, duplicate, empty, oversized and missing-shard workloads are rejected', () => {
  for (const [all, shards, options] of [
    [[], [[], []]],
    [['a', 'a'], [['a'], ['a']]],
    [['a', 'b', 'c'], [['a'], ['b']]],
    [['a', 'b'], [['a'], ['other']]],
    [['a', 'b'], [['a'], ['a', 'b']]],
    [['a', 'b'], [[], ['a', 'b']]],
    [['a', 'b', 'c'], [['a', 'b'], ['c']], { ceiling: 1 }],
    [['a', 'b'], [['a', 'b']]],
  ]) assert.throws(() => verifyThemePartitions(all, shards, { count: 2, ceiling: 3, ...options }));
});

test('workflow, wrapper and three independent matrices agree on the bounded scheduling plan', () => {
  assert.equal(WINDOWS_THEME_SHARDS, 8); assert.equal(MAX_WINDOWS_THEME_TESTS, 160);
  const ci = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const job = ci.split('  visual-windows-themes:\n')[1];
  assert.match(job, /shard: \[1, 2, 3, 4, 5, 6, 7, 8\]/);
  assert.match(job, /Verify exact Windows theme shard coverage\n        shell: pwsh\n        run: node scripts\/check-windows-theme-shards\.mjs\n      - name: Build visual fixture before the server startup deadline/);
  const script = readFileSync(new URL('../../scripts/windows-visual-afd.ps1', import.meta.url), 'utf8');
  assert.ok(script.includes("$env:THEME_SHARD -match '^[1-8]$'"));
  assert.ok(script.includes('--shard "$env:THEME_SHARD/8"'));
  const guard = readFileSync(new URL('../../scripts/check-windows-theme-shards.mjs', import.meta.url), 'utf8');
  assert.ok(guard.includes("'--list', '--forbid-only', '--reporter=json'"));
  for (const file of ['file-states', 'public-page-chrome', 'public-viewport']) {
    const source = readFileSync(new URL(`../themes/${file}.spec.js`, import.meta.url), 'utf8');
    assert.equal(source.match(/test\.describe\.configure\(\{ mode: 'parallel' \}\)/g)?.length, 1);
    assert.ok(!/test\.(?:beforeAll|afterAll)|describe\.serial/.test(source));
  }
});
