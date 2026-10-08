import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

export function verifyNativeReport(report) {
  assert.equal(report.stats.expected, 41);
  for (const key of ['skipped', 'unexpected', 'flaky']) assert.equal(report.stats[key], 0, key);
  assert.deepEqual(report.errors, []);
  const tests = [];
  const visit = suite => {
    for (const spec of suite.specs || []) tests.push(...spec.tests);
    for (const child of suite.suites || []) visit(child);
  };
  for (const suite of report.suites) visit(suite);
  assert.equal(tests.length, 41);
  for (const test of tests) {
    assert.equal(test.expectedStatus, 'passed');
    assert.equal(test.status, 'expected');
    assert.equal(test.results.length, 1);
    assert.equal(test.results[0].status, 'passed');
    assert.equal(test.results[0].retry, 0);
  }
  return { nativeTests: tests.length, attemptsPerTest: 1, skipped: 0, retries: 0 };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv.length, 3);
  const bytes = readFileSync(process.argv[2]);
  assert(bytes.length <= 4 * 1024 * 1024, 'Native report exceeds evidence limit');
  console.log(JSON.stringify(verifyNativeReport(JSON.parse(bytes.toString('utf8')))));
}
