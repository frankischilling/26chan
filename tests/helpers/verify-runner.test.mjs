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
const browserPrerequisites = [prerequisites[0], prerequisites[1], prerequisites[4], prerequisites[5]];
const dependencyCommands = ['npm ci --ignore-scripts', 'npx playwright install --with-deps chromium'];
const suites = ['rust', 'browser-content', 'browser-interactions'];
const roles = ['TEST_PUBLIC', 'MEDIA', 'MEDIA_READ', 'INTAKE', 'MONITOR', 'AUTH', 'STAFF'];
const staff = 'npx playwright test --config playwright.staff.config.js';
// Captured before splitting verification into CI lanes. Keep this expectation
// independent of the runner's functions so a removed command cannot disappear
// from both the full sequence and the combined lane coverage unnoticed.
// Timeout doubles also record the command they execute.
const originalCommands = `python3 scripts/extract-preview-policy-reference.py
python3 scripts/test-preview-policy-reference.py
python3 scripts/extract-quote-resolution-reference.py
python3 scripts/test-quote-resolution-reference.py
python3 scripts/extract-navigation-reference.py
python3 scripts/test-navigation-reference.py
python3 scripts/extract-board-format-metadata-reference.py
python3 scripts/test-board-format-metadata-reference.py
npm ci --ignore-scripts
npx playwright install --with-deps chromium
npm run check:generated
timeout 300s npm run test:math
npm run test:math
python3 scripts/check-windows-test-exits.py
npm run test:settings-categories
node --test tests/browser/staff-auth-budget.test.mjs tests/browser/owned-upload-response.test.mjs tests/browser/deletion-quota-fixture.test.mjs tests/helpers/verify-runner.test.mjs
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
python3 scripts/check-media-parser-dependencies.py
cargo build --workspace --examples --bins --locked
cargo build -p board-public --example deletion-quota-fixture --features browser-tests --locked
cargo test --workspace --all-features --locked
cargo test -p board-staff --features database-tests --test uploads --locked script_disabled_browser -- --ignored --exact --nocapture
cargo test -p board-public --example deletion-fixture --features database-tests --locked
cargo test -p board-public --example deletion-quota-fixture --features browser-tests --locked
timeout 300s npm run test:drawing
npm run test:drawing
npx playwright test tests/browser/anonymous-session.spec.js
node --test tests/browser/polls-fixture.test.mjs
npm run test:polls
npm run test:blotter
npm run test:blotter-persisted
npm run test:page-identity-core
npm run test:board-subtitles
npm run test:global-search
npm run test:posting-randomizers
npm run test:robot9000
npm run test:wordfilters
npm run test:linkification
npx playwright test tests/browser/static-quotes.spec.js tests/browser/quote-resolution.spec.js
npm run test:quote-preview
npm run test:backlinks
npm run test:inline-quotes
npm run test:images-core
npm run test:files-core
npm run test:custom-spoilers
npm run test:source-flags
npm run test:display
npm run test:post-tooltips
npm run check:native-thread-controls
npm run test:source-parsing
npm run test:thread-updater-dom
npm run test:expansion
npm run test:stats
npm run test:navigation
npm run test:layout
npm run test:depager
npm run test:embeds
npm run test:custom-css
npm run test:settings-transfer
npx playwright test --config playwright.media-visual.config.js tests/media-visual/native-images.spec.js
npx playwright test --config playwright.media-visual.config.js tests/media-visual/file-presentation.spec.js
npm run test:quick-reply
npx playwright test tests/browser/mobile-post-headers.spec.js
npx playwright test tests/browser/post-identities.spec.js
npx playwright test tests/browser/poster-ids.spec.js
npm run test:catalog-filters-core
npm run test:catalog-theme-core
npm run test:behavior
npx playwright test tests/browser/catalog-teasers.spec.js
npx playwright test tests/browser/text-catalog.spec.js
npx playwright test tests/browser/catalog-previews.spec.js
npx playwright test --config playwright.staff.config.js`.split('\n');
// verify.sh belongs to the Linux CI lane. No Bash dependency is added to Windows.
const options = { skip: process.platform === 'win32' };

function runRunner(t, { fail = '', source, args = [], env: overrides = {} } = {}) {
  const directory = mkdtempSync(path.join(tmpdir(), 'verify-runner-order-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const bin = path.join(directory, 'bin');
  const log = path.join(directory, 'commands');
  const environmentLog = path.join(directory, 'environment');
  mkdirSync(bin);
  writeFileSync(log, '');
  writeFileSync(environmentLog, '');
  // Command-trace doubles only: no builds, databases, fixtures, or browsers run.
  // PATH contains only these owned stubs, and no caller credentials are inherited.
  const stub = `#!/bin/bash
set -euo pipefail
name=\${0##*/}
command="$name $*"
printf '%s\\n' "$command" >> "$VERIFY_TEST_LOG"
printf '%s\\t%s\\t%s\\t%s\\n' "$command" "\${AZURE_CONFIG_DIR-}" "\${SSH_AUTH_SOCK-}" "\${EXTRA_DATABASE_URL-}" >> "$VERIFY_TEST_ENV_LOG"
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
  const env = { PATH: bin, APP_ENV: 'development', CI: 'true', VERIFY_TEST_LOG: log, VERIFY_TEST_ENV_LOG: environmentLog, VERIFY_TEST_FAIL: fail };
  for (const role of roles) {
    env[`${role}_DATABASE_URL`] = 'owned-command-trace-only';
  }
  for (const [key, value] of Object.entries(overrides)) {
    if (value === undefined) delete env[key];
    else env[key] = value;
  }
  const result = spawnSync('/bin/bash', ['--noprofile', '--norc', script, ...args], {
    cwd: directory, env, encoding: 'utf8', timeout: 5000, maxBuffer: 65536,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  return {
    ...result,
    commands: readFileSync(log, 'utf8').trim().split('\n').filter(Boolean),
    environments: readFileSync(environmentLog, 'utf8').split('\n').filter(Boolean).map(line => line.split('\t')),
  };
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

test('default and explicit all preserve the exact original command sequence', options, t => {
  for (const args of [[], ['all']]) {
    const result = runRunner(t, { args });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(result.commands, originalCommands);
  }
});

test('the original sequence assertion detects a command removed from every mode', options, t => {
  const source = readFileSync(runner, 'utf8').replace('  npm run test:polls\n', '');
  assert.notEqual(source, readFileSync(runner, 'utf8'));
  const result = runRunner(t, { source });
  assert.equal(result.status, 0, result.stderr);
  assert.ok(!result.commands.includes('npm run test:polls'));
  assert.throws(() => assert.deepEqual(result.commands, originalCommands), { code: 'ERR_ASSERTION' });
});

test('CI suites cover the full runner with only repeated dependency and fixture preparation', options, t => {
  const full = runRunner(t);
  assert.equal(full.status, 0, full.stderr);
  const results = suites.map(suite => runRunner(t, { args: [suite] }));
  for (const result of results) assert.equal(result.status, 0, result.stderr);
  // All three runners install dependencies and prepare the same four fixtures.
  // Every other command must occur with exactly its full-run multiplicity.
  const repeated = [...dependencyCommands, ...browserPrerequisites];
  assert.deepEqual(
    results.flatMap(result => result.commands).sort(),
    [...full.commands, ...repeated, ...repeated].sort(),
  );
  const rust = results[0].commands;
  const installed = position(rust, dependencyCommands[1]);
  for (const prerequisite of prerequisites) {
    assert.ok(installed < position(rust, prerequisite), `Rust browser qualification requires Chromium: ${prerequisite}`);
  }
  assert.ok(!rust.includes(drawing));
  assert.ok(!rust.includes('npm run test:behavior'));
});

test('each browser suite installs dependencies and builds and tests fixtures before its first workload', options, t => {
  for (const [suite, first] of [
    ['browser-content', 'timeout 300s npm run test:math'],
    ['browser-interactions', 'npm run test:display'],
  ]) {
    const result = runRunner(t, { args: [suite] });
    assert.equal(result.status, 0, result.stderr);
    const preparation = [...dependencyCommands, ...browserPrerequisites];
    assert.deepEqual(result.commands.slice(0, preparation.length), preparation, suite);
    assert.equal(position(result.commands, first), preparation.length, suite);
    for (const prerequisite of preparation) {
      assert.ok(position(result.commands, prerequisite) < position(result.commands, first), `${suite}: ${prerequisite}`);
    }
  }
});

test('local suites retain their separately installed browser prerequisite', options, t => {
  for (const suite of ['all', ...suites]) {
    const result = runRunner(t, { args: [suite], env: { CI: undefined } });
    assert.equal(result.status, 0, result.stderr);
    position(result.commands, dependencyCommands[0]);
    assert.ok(!result.commands.includes(dependencyCommands[1]), suite);
  }
  const result = runRunner(t, { args: ['rust'], env: { CI: 'false' } });
  assert.equal(result.status, 0, result.stderr);
  assert.ok(!result.commands.includes(dependencyCommands[1]));
});

test('unknown suites and extra arguments refuse before invoking any command', options, t => {
  for (const args of [['unknown'], [''], ['all', 'rust'], ['browser-content', 'unexpected']]) {
    const result = runRunner(t, { args });
    assert.equal(result.status, 2, `${JSON.stringify(args)}: ${result.stderr}`);
    assert.match(result.stderr, /Usage: scripts\/verify\.sh/);
    assert.deepEqual(result.commands, []);
  }
});

test('every suite requires development mode and every disposable role before invoking commands', options, t => {
  for (const suite of ['all', ...suites]) {
    const production = runRunner(t, { args: [suite], env: { APP_ENV: 'production' } });
    assert.equal(production.status, 1, production.stderr);
    assert.match(production.stderr, /Verification requires development mode\./);
    assert.deepEqual(production.commands, []);
    for (const role of roles) {
      const key = `${role}_DATABASE_URL`;
      for (const value of [undefined, '']) {
        const result = runRunner(t, { args: [suite], env: { [key]: value } });
        assert.equal(result.status, 1, `${suite}: ${key}: ${result.stderr}`);
        assert.ok(result.stderr.includes(key));
        assert.deepEqual(result.commands, [], `${suite}: ${key}`);
      }
    }
  }
});

test('CI suites preserve failures and never execute a command after a failed prerequisite or workload', options, t => {
  for (const [suite, failures] of [
    ['rust', [
      ...dependencyCommands, ...prerequisites,
      'python3 scripts/extract-preview-policy-reference.py',
      'npm run check:generated',
      'cargo clippy --workspace --all-targets --all-features --locked -- -D warnings',
    ]],
    ['browser-content', [
      ...dependencyCommands, ...browserPrerequisites,
      'timeout 300s npm run test:math', 'npm run test:math',
      boundedDrawing, drawing, 'npm run test:source-flags',
    ]],
    ['browser-interactions', [
      ...dependencyCommands, ...browserPrerequisites,
      'npm run test:display', 'npm run test:behavior', staff,
    ]],
  ]) {
    const successful = runRunner(t, { args: [suite] });
    assert.equal(successful.status, 0, successful.stderr);
    for (const fail of failures) {
      const expected = successful.commands.slice(0, position(successful.commands, fail) + 1);
      const result = runRunner(t, { args: [suite], fail });
      assert.equal(result.status, 73, `${suite}: ${fail}: ${result.stderr}`);
      assert.deepEqual(result.commands, expected, `${suite}: stop at ${fail}`);
    }
  }
});

test('the staff browser alone receives all three synthetic inherited-environment probes', options, t => {
  for (const suite of ['all', ...suites]) {
    const result = runRunner(t, { args: [suite] });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.environments.length, result.commands.length);
    for (const [command, ...environment] of result.environments) {
      assert.deepEqual(environment, command === staff ? [
        '/tmp/owned-synthetic-cloud-config', '/tmp/owned-synthetic-agent', 'owned-synthetic-shadow-url',
      ] : ['', '', ''], `${suite}: ${command}`);
    }
  }
});
