import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const read = path => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');
const ci = read('.github/workflows/ci.yml');
function workflowJob(name) {
  const matches = ci.split(`  ${name}:\n`);
  assert.equal(matches.length, 2, `Exactly one workflow job: ${name}`);
  return matches[1].split(/^  [a-z0-9-]+:\n/m)[0];
}
const job = workflowJob('visual-windows-core');
const steps = job.split('      - name: ').slice(1);
const step = name => {
  const matches = steps.filter(value => value.startsWith(`${name}\n`));
  assert.equal(matches.length, 1, name);
  return matches[0];
};
const suites = [
  ['Run deterministic visual fixture without a database service', 'test:visual', 'windows-core-visual'],
  ['Run archive desktop and mobile visual fixtures', 'test:archive-visual', 'windows-archive-visual'],
  ['Run attachment desktop and mobile visual fixtures', 'test:media-visual', 'windows-media-visual'],
  ['Run public empty and error desktop/mobile states', 'test:states', 'windows-public-states'],
];
const condition = source => source.match(/^        if: \$\{\{ (.+) \}\}$/m)?.[1];

// Evaluate the checked-in expression, including Actions' implicit success guard
// when no status function is present. No browser or fixture execution is needed.
function scheduled(source, { cancelled = false, previous = 'success', install = 'success', build = 'success' } = {}) {
  const expression = condition(source);
  assert.ok(expression, 'a full-page suite needs an explicit condition');
  const hasStatus = /\b(?:success|failure|cancelled|always)\(\)/.test(expression);
  return (!hasStatus ? previous === 'success' : true) && runInNewContext(expression, {
    cancelled: () => cancelled,
    success: () => previous === 'success',
    failure: () => previous === 'failure',
    always: () => true,
    steps: { browser_dependencies: { outcome: install }, visual_fixture: { outcome: build } },
  }, { timeout: 100 });
}

// Interpret only the checked-in aggregate's literal Bash test commands. Reject
// other shell syntax rather than ignoring it; this keeps the status checks
// executable on Windows without adding a Bash dependency to these helper tests.
function aggregateResult(source, results) {
  const environment = Object.fromEntries([...source.matchAll(
    /^          ([A-Z_]+): \$\{\{ needs\.([a-z0-9-]+)\.result \}\}$/gm,
  )].map(([, name, dependency]) => [name, results[dependency]]));
  const commands = source.match(/^        run: \|\n((?:          [^\n]*(?:\n|$))+)/m)?.[1];
  assert.ok(commands, 'the aggregate must contain its checked-in run block');
  return commands.trim().split('\n').map(line => {
    const comparison = line.trim().match(/^test "\$([A-Z_]+)" = success$/);
    assert.ok(comparison, `Unrecognized aggregate command: ${line.trim()}`);
    return environment[comparison[1]] === 'success';
  }).every(Boolean);
}

function assertAggregateOutcomes(source, dependencies, results = {}) {
  const remaining = dependencies.filter(dependency => !Object.hasOwn(results, dependency));
  if (remaining.length === 0) {
    assert.equal(
      aggregateResult(source, results),
      dependencies.every(dependency => results[dependency] === 'success'),
      `aggregate gate outcome: ${JSON.stringify(results)}`,
    );
    return;
  }
  for (const status of ['success', 'failure', 'cancelled', 'skipped', '']) {
    assertAggregateOutcomes(source, dependencies, { ...results, [remaining[0]]: status });
  }
}

test('each independent suite requires installed browsers and a built fixture, even after another failure', () => {
  const install = step('Install browser dependencies');
  const build = step('Build visual fixture before the server startup deadline');
  assert.match(install, /\n        id: browser_dependencies\n/);
  assert.match(install, /npm ci --ignore-scripts\n          if \(\$LASTEXITCODE -ne 0\) \{ exit \$LASTEXITCODE \}\n          npx playwright install chromium\n/);
  assert.match(build, /\n        id: visual_fixture\n/);
  assert.match(build, /run: cargo build -p board-public --example visual-fixtures --locked\n/);
  assert.ok(!/^        if:/m.test(install + build));
  for (const [name] of suites) {
    const source = step(name);
    assert.ok(job.indexOf(build) < job.indexOf(source));
    for (const previous of ['success', 'failure', 'skipped']) {
      assert.equal(scheduled(source, { previous }), true, `${name}: previous ${previous}`);
      assert.equal(scheduled(source, { previous, cancelled: true }), false, `${name}: cancellation`);
      for (const outcome of ['failure', 'cancelled', 'skipped', '']) {
        assert.equal(scheduled(source, { previous, install: outcome }), false, `${name}: install ${outcome}`);
        assert.equal(scheduled(source, { previous, build: outcome }), false, `${name}: build ${outcome}`);
      }
    }
  }
});

test('screenshot failures stay fatal while the Windows core starts independently', () => {
  assert.ok(!/continue-on-error|--update-snapshots|--retries|--workers|\|\|\s*true|exit 0/.test(job));
  assert.ok(!/^    (?:needs|if):/m.test(job));
  assert.match(job, /timeout-minutes: 30\n/);
  // Each command remains the last command in its step, so pwsh propagates its exit status.
  for (const [name, command, output] of suites) {
    assert.ok(step(name).trimEnd().endsWith(`npm run ${command} -- --output=test-results/${output}`));
  }
});

test('both required aggregate checks reject every failed, cancelled, skipped or absent child result', () => {
  for (const [name, dependencies] of [
    ['visual-windows', ['visual-windows-core', 'visual-windows-themes']],
    ['rust-and-postgres', ['rust-and-browser', 'browser-suites', 'media-and-operations']],
  ]) {
    const source = workflowJob(name);
    assert.equal(source.match(/^    needs: \[([^\]]+)\]$/m)?.[1], dependencies.join(', '));
    assert.match(source, /^    if: always\(\)$/m);
    assert.match(source, /^    runs-on: ubuntu-24\.04$/m);
    assert.match(source, /^    timeout-minutes: 5$/m);
    assert.equal(source.match(/^      - name:/gm)?.length, 1);
    assert.ok(!/continue-on-error|shell:|^        if:/m.test(source));
    assertAggregateOutcomes(source, dependencies);
  }
});

test('aggregate status assertions detect an omitted comparison and a result mapped to the wrong child', () => {
  const source = workflowJob('rust-and-postgres');
  const dependencies = ['rust-and-browser', 'browser-suites', 'media-and-operations'];
  const omitted = source.replace('          test "$BROWSER_RESULT" = success\n', '');
  assert.notEqual(omitted, source);
  assert.throws(() => assertAggregateOutcomes(omitted, dependencies), /aggregate gate outcome:/);
  const wrongChild = source.replace('needs.browser-suites.result', 'needs.rust-and-browser.result');
  assert.notEqual(wrongChild, source);
  assert.throws(() => assertAggregateOutcomes(wrongChild, dependencies), /aggregate gate outcome:/);
});

test('the required Linux browser matrix runs both complete suites independently', () => {
  const browser = workflowJob('browser-suites');
  assert.equal(browser.match(/^        suite: \[([^\]]+)\]$/m)?.[1], 'content, interactions');
  assert.match(browser, /^      fail-fast: false$/m);
  assert.ok(!/^    (?:needs|if):|continue-on-error|^        (?:include|exclude):/m.test(browser));
  assert.ok(browser.includes('BROWSER_SUITE: ${{ matrix.suite }}'));
  assert.ok(browser.includes('bash scripts/verify.sh "browser-$BROWSER_SUITE"'));
  assert.ok(workflowJob('rust-and-browser').includes('bash scripts/verify.sh rust\n'));
});

test('the renamed Windows core writes the existing cache namespace restored by every theme shard', () => {
  const themes = workflowJob('visual-windows-themes');
  const sharedKey = "windows-2025-${{ hashFiles('Cargo.toml') }}-visual-windows";
  assert.equal(job.match(/^          shared-key: (.+)$/m)?.[1], sharedKey);
  assert.equal(themes.match(/^          shared-key: (.+)$/m)?.[1], sharedKey);
  assert.match(themes, /^          save-if: false$/m);
  assert.ok(!/^          save-if: false$/m.test(job));
  assert.ok(!/^    (?:needs|if):/m.test(themes));
});

test('four sibling output paths preserve every suite under the existing bounded artifact upload', () => {
  const paths = suites.map(([name, command, output]) => {
    const source = step(name);
    assert.equal(source.match(/npm run /g)?.length, 1);
    assert.ok(source.includes(`npm run ${command} -- --output=test-results/${output}\n`));
    return source.match(/--output=(\S+)/)[1];
  });
  assert.equal(new Set(paths).size, 4);
  for (const path of paths) {
    assert.match(path, /^test-results\/windows-[a-z-]+$/);
    assert.ok(!paths.some(other => other !== path && other.startsWith(`${path}/`)));
  }
  const upload = step('Retain synthetic Windows visual failure diagnostics');
  assert.match(upload, /\n        if: failure\(\)\n/);
  for (const glob of ['test-results/**/*.png', 'test-results/**/trace.zip', 'test-results/**/error-context.md', 'test-results/**/netlogs/*.json']) {
    assert.ok(upload.includes(`            ${glob}\n`));
  }
  assert.match(upload, /retention-days: 3\n/);
  assert.match(upload, /include-hidden-files: false\n/);
  assert.match(step('Verify independent Windows visual evidence scheduling'), /run: node --test tests\/helpers\/windows-visual-evidence.test.mjs\n/);
});

test('all four suites own their fixture server and preserve strict visual settings', async () => {
  const previous = process.env.VISUAL_FIXTURE_SERVER;
  process.env.VISUAL_FIXTURE_SERVER = '1';
  try {
    const scripts = JSON.parse(read('package.json')).scripts;
    for (const [index, config] of ['playwright.config.js', 'playwright.archive-visual.config.js', 'playwright.media-visual.config.js', 'playwright.states.config.js'].entries()) {
      const { default: value } = await import(new URL(`../../${config}`, import.meta.url));
      assert.equal(scripts[suites[index][1]], index === 0 ? 'playwright test tests/browser/visual.spec.js' : `playwright test --config ${config}`);
      assert.equal(value.workers, 1);
      assert.equal(value.retries, 0);
      assert.equal(value.fullyParallel, false);
      assert.equal(value.expect.toHaveScreenshot.maxDiffPixels, 0);
      assert.equal(value.use.trace, 'retain-on-failure');
      assert.equal(value.webServer.reuseExistingServer, false);
      assert.equal(value.webServer.command, 'cargo run -p board-public --example visual-fixtures --locked');
      assert.equal(value.globalSetup, undefined);
      assert.equal(value.globalTeardown, undefined);
    }
  } finally {
    if (previous === undefined) delete process.env.VISUAL_FIXTURE_SERVER;
    else process.env.VISUAL_FIXTURE_SERVER = previous;
  }
});
