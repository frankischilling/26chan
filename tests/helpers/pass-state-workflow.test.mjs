import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const workflow = readFileSync(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
const job = workflow.split('  rust-and-browser:\n')[1].split(/^  [a-z][a-z-]+:\n/m)[0];
function checkOrder(job) {
  const npm = job.indexOf('npm ci --ignore-scripts');
  const browser = job.indexOf('npx playwright install --with-deps chromium');
  const matrix = job.indexOf('cargo test -p board-public --all-features --locked --test pass_state_matrix');
  assert.ok(npm >= 0 && browser > npm && matrix > browser);
  assert.equal(job.match(/npm ci --ignore-scripts/g).length, 1);
  assert.match(job.slice(browser, matrix), /source \.local\/verification-tests\.env/);
  assert.match(job.slice(matrix).split('\n')[0], /-- --ignored --test-threads=1$/);
}
test('Pass browser state qualification installs Node and Chromium dependencies first', () => checkOrder(job));
test('the old late browser installation is rejected', () => {
  const late = job.replace('          npm ci --ignore-scripts\n          npx playwright install --with-deps chromium\n', '') + '\n          npm ci --ignore-scripts\n          npx playwright install --with-deps chromium\n';
  assert.throws(() => checkOrder(late));
});
