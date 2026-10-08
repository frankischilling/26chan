import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const runner = fileURLToPath(new URL('../../scripts/verify.sh', import.meta.url));
const drawing = 'npm run test:drawing';
const boundedDrawing = `timeout 300s ${drawing}`;
const prerequisites = [
  'cargo build --workspace --examples --bins --locked',
  'cargo build -p board-public --example deletion-quota-fixture --features browser-tests --locked',
  'cargo test --workspace --all-features --locked',
  'cargo test -p board-staff --features database-tests --test uploads --locked script_disabled_browser -- --ignored --exact --nocapture',
  'cargo test -p board-public --example deletion-fixture --features database-tests --locked',
  'cargo test -p board-public --example deletion-quota-fixture --features browser-tests --locked',
];
// verify.sh belongs to the Linux CI lane. No Bash dependency is added to Windows.
const options = { skip: process.platform === 'win32' };

function runRunner(t, { fail = '', source } = {}) {
  const directory = mkdtempSync(path.join(tmpdir(), 'verify-runner-order-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const bin = path.join(directory, 'bin');
  const log = path.join(directory, 'commands');
  mkdirSync(bin);
  writeFileSync(log, '');
  // Command-trace doubles only: no builds, databases, fixtures, or browsers run.
  // PATH contains only these owned stubs, and no caller credentials are inherited.
  const stub = `#!/bin/bash
set -euo pipefail
name=\${0##*/}
command="$name $*"
printf '%s\\n' "$command" >> "$VERIFY_TEST_LOG"
if [[ "$command" == "$VERIFY_TEST_FAIL" ]]; then exit 73; fi
if [[ "$name" == timeout ]]; then
  shift
  exec "$@"
fi
exit 0
`;
  for (const name of ['npm', 'npx', 'node', 'python3', 'cargo', 'timeout']) {
    writeFileSync(path.join(bin, name), stub, { mode: 0o700 });
  }
  let script = runner;
  if (source !== undefined) {
    script = path.join(directory, 'old-order.sh');
    writeFileSync(script, source);
  }
  const env = { PATH: bin, APP_ENV: 'development', CI: 'true', VERIFY_TEST_LOG: log, VERIFY_TEST_FAIL: fail };
  for (const role of ['TEST_PUBLIC', 'MEDIA', 'MEDIA_READ', 'INTAKE', 'MONITOR', 'AUTH', 'STAFF']) {
    env[`${role}_DATABASE_URL`] = 'owned-command-trace-only';
  }
  const result = spawnSync('/bin/bash', ['--noprofile', '--norc', script], {
    cwd: directory, env, encoding: 'utf8', timeout: 5000, maxBuffer: 65536,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  return { ...result, commands: readFileSync(log, 'utf8').trim().split('\n').filter(Boolean) };
}

function position(commands, command) {
  assert.equal(commands.filter(value => value === command).length, 1, `Run exactly once: ${command}`);
  return commands.indexOf(command);
}

function assertDrawingOrder(commands) {
  const start = position(commands, boundedDrawing);
  for (const prerequisite of prerequisites) {
    assert.ok(position(commands, prerequisite) < start, `Drawing must follow: ${prerequisite}`);
  }
  assert.equal(position(commands, drawing), start + 1);
  assert.ok(start < position(commands, 'npx playwright test tests/browser/anonymous-session.spec.js'));
}

test('real verification runner builds and tests fixtures before the bounded drawing command', options, t => {
  const result = runRunner(t);
  assert.equal(result.status, 0, result.stderr);
  assertDrawingOrder(result.commands);
  const build = position(result.commands, prerequisites[0]);
  assert.ok(position(result.commands, 'timeout 300s npm run test:math') < build);
  assert.ok(position(result.commands, 'npm run test:settings-categories') < build);
  position(result.commands, 'npx playwright install --with-deps chromium');
  position(result.commands, 'npx playwright test --config playwright.staff.config.js');
});

test('the command-order assertion rejects the former early drawing invocation', options, t => {
  const source = readFileSync(runner, 'utf8').replace(`${boundedDrawing}\n`, '')
    .replace('timeout 300s npm run test:math\n', `timeout 300s npm run test:math\n${boundedDrawing}\n`);
  const result = runRunner(t, { source });
  assert.equal(result.status, 0, result.stderr);
  assert.throws(() => assertDrawingOrder(result.commands), /Drawing must follow:/);
});

test('real verification runner stops on every fixture prerequisite or drawing failure', options, t => {
  for (const fail of [...prerequisites, drawing]) {
    const result = runRunner(t, { fail });
    assert.equal(result.status, 73, `Preserve failure status: ${fail}\n${result.stderr}`);
    assert.equal(result.commands.at(-1), fail, `No command may follow failure: ${fail}`);
    assert.ok(!result.commands.includes('npx playwright test tests/browser/anonymous-session.spec.js'));
    if (fail !== drawing) assert.ok(!result.commands.includes(boundedDrawing));
    else position(result.commands, boundedDrawing);
  }
});
