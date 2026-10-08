import { test } from 'node:test';
import assert from 'node:assert/strict';
import { verifyNativeReport } from './verify-native-report.mjs';
const good = () => ({ stats: { expected: 41, skipped: 0, unexpected: 0, flaky: 0 }, errors: [],
  suites: [{ suites: [{ specs: [{ tests: Array.from({ length: 41 }, () => ({ expectedStatus: 'passed',
    status: 'expected', results: [{ status: 'passed', retry: 0 }] })) }] }] }] });
const first = report => report.suites[0].suites[0].specs[0].tests[0];
test('accepts exactly one passing attempt for all 41 native tests', () => {
  assert.equal(verifyNativeReport(good()).nativeTests, 41);
});
for (const [name, mutate] of [
  ['missing case', r => r.suites[0].suites[0].specs[0].tests.pop()],
  ['unexpected extra case', r => r.suites[0].suites[0].specs[0].tests.push(first(r))],
  ['skipped aggregate', r => r.stats.skipped++],
  ['failed aggregate', r => r.stats.unexpected++],
  ['flaky aggregate', r => r.stats.flaky++],
  ['expected failure annotation', r => first(r).expectedStatus = 'failed'],
  ['discovery without execution', r => first(r).results = []],
  ['retry hidden in aggregate', r => first(r).results[0].retry = 1],
  ['second attempt', r => first(r).results.push({ status: 'passed', retry: 1 })],
  ['skipped result hidden in aggregate', r => first(r).results[0].status = 'skipped'],
  ['unexpected result hidden in aggregate', r => first(r).status = 'unexpected'],
  ['top-level failure', r => r.errors.push({ message: 'worker failed' })],
]) test(`rejects ${name}`, () => { const report = good(); mutate(report); assert.throws(() => verifyNativeReport(report)); });

test('runner-dependent target is initialized by a runner step, not job-level env', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = await readFile(new URL('../../../.github/workflows/native-replay-qualification.yml', import.meta.url), 'utf8');
  const jobEnv = source.split('    env:\n')[1]?.split('    steps:\n')[0];
  assert(jobEnv, 'Expected explicit job environment');
  assert.doesNotMatch(jobEnv, /\$\{\{\s*runner\./);
  assert.match(source, /REPLAY_PROBE_CARGO_TARGET_DIR=%s\/replay-worker-probe-target\\n' "\$RUNNER_TEMP" >> "\$GITHUB_ENV"/);
});
