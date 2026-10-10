import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { armName, binding, contrastEnabled, checkEnvironment, contrastLaunchOptions, DEFAULT_DISABLED,
  DEFAULT_ENABLED, FEATURE, BROWSER_VERSION, featureLists, verifyCommandLine, validateManifest, qualifyPair,
  runnerIdentity } from '../../scripts/windows-port-contrast-core.mjs';
import { contrastServerCommand, portContrastOptions } from './windows-port-contrast.js';

const run = { head: 'a'.repeat(40), run: '12345', attempt: 1 };
const env = { CANDIDATE_SHA: run.head, GITHUB_RUN_ID: run.run, GITHUB_RUN_ATTEMPT: '1', WINDOWS_PORT_CONTRAST: '1', WINDOWS_PORT_ARM: 'enabled',
  THEME_SHARD: '7', WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS: '1', WINDOWS_VISUAL_NETLOG: '1' };
function command(arm) {
  const lists = featureLists(arm);
  return ['PRIVATE_EXECUTABLE', '--enable-automation', '--headless', '--disable-field-trial-config',
    `--enable-features=${lists.enabled.join(',')}`, `--disable-features=${lists.disabled.join(',')}`,
    '--no-sandbox', '--hide-scrollbars', '--mute-audio', '--no-startup-window', '--remote-debugging-pipe', '--user-data-dir=PRIVATE_PROFILE'];
}
const runner = { image_os: 'win25', image_version: '20261001.50.1', windows_build: '26100.1234', release: '10.0.26100', arch: 'x64', node: 'v24.14.0' };
function evidence(arm, failed = 0) {
  const proof = verifyCommandLine(command(arm), arm, BROWSER_VERSION);
  return { schema: 1, binding: run, arm, complete: true, fixture_verified: true, discovery_verified: true,
    fixture_sha256: 'b'.repeat(64), fixture_bytes: 100, browser_sha256: 'c'.repeat(64), order_sha256: 'd'.repeat(64),
    playwright: '1.62.0', browser_version: BROWSER_VERSION, runner, test_count: 148, executed: 148, skipped: 0, retries: 0, failed,
    browser_count: 1, proof_count: 1, browsers: [{ worker: 0, ...proof }], all_command_lines_verified: true,
    feature_enabled: arm === 'enabled', feature_disabled: arm === 'disabled', non_target_features_preserved: true,
    socket_option_success: 'unobserved', netlog: { schema: 1, status: failed ? 'complete' : 'absent', files: failed ? 1 : 0, command_line_redactions: 0, event_chains_preserved: true, captures: failed ? [{ ordinal: 1, bytes: 100, sha256: 'f'.repeat(64), command_line_redacted: false }] : [] }, original: { kind: 'exit', code: failed ? 1 : 0, signal: null } };
}

for (const arm of ['enabled', 'disabled']) {
  test(`${arm}: exactly one changed original switch is removed and the replacement survives Playwright filtering`, () => {
    const options = contrastLaunchOptions({}, arm);
    const defaults = [`--enable-features=${DEFAULT_ENABLED.join(',')}`, `--disable-features=${DEFAULT_DISABLED.join(',')}`, '--headless'];
    const final = [...defaults, ...options.args].filter(arg => !options.ignoreDefaultArgs.includes(arg));
    assert.equal(options.ignoreDefaultArgs.length, 1);
    assert.equal(final.filter(arg => arg.startsWith('--enable-features=')).length, 1);
    assert.equal(final.filter(arg => arg.startsWith('--disable-features=')).length, 1);
    assert.ok(final.includes('--headless'));
    const expected = featureLists(arm);
    assert.ok(final.includes(`--enable-features=${expected.enabled.join(',')}`));
    assert.ok(final.includes(`--disable-features=${expected.disabled.join(',')}`));
    assert.ok(!options.ignoreDefaultArgs.includes(options.args[0]));
  });
  test(`${arm}: observed normalized features preserve every existing feature without retaining arguments`, () => {
    const args = command(arm), before = JSON.stringify(args);
    const proof = verifyCommandLine(args, arm, BROWSER_VERSION);
    assert.equal(proof.command_line_verified, true);
    assert.equal(proof.socket_option_success, 'unobserved');
    assert.equal(JSON.stringify(args), before);
    assert.ok(!JSON.stringify(proof).includes('PRIVATE'));
    assert.ok(!JSON.stringify(proof).includes('--'));
    const enabledIndex = args.findIndex(arg => arg.startsWith('--enable-features='));
    const disabledIndex = args.findIndex(arg => arg.startsWith('--disable-features='));
    for (const index of [enabledIndex, disabledIndex]) {
      const missing = [...args]; missing.splice(index, 1);
      assert.throws(() => verifyCommandLine(missing, arm, BROWSER_VERSION));
      const duplicate = [...args, args[index]];
      assert.throws(() => verifyCommandLine(duplicate, arm, BROWSER_VERSION));
      const drift = [...args]; drift[index] += ',UnexpectedFeature';
      assert.throws(() => verifyCommandLine(drift, arm, BROWSER_VERSION));
    }
    assert.throws(() => verifyCommandLine(args, arm, '151.0.7922.35'));
    assert.throws(() => verifyCommandLine([...args, '--force-fieldtrials=Private'], arm, BROWSER_VERSION));
    assert.throws(() => verifyCommandLine([...args, '--enable-features'], arm, BROWSER_VERSION));
  });
}
test('disabling only the target must retain all original disabled features', () => {
  const args = command('disabled').map(arg => arg.startsWith('--disable-features=') ? `--disable-features=${FEATURE}` : arg);
  assert.throws(() => verifyCommandLine(args, 'disabled', BROWSER_VERSION), error => !error.message.includes('PRIVATE'));
});
test('duplicate, parametrized, conflicting and unsupported feature overrides are rejected', () => {
  for (const suffix of [`,${FEATURE}`, `<${FEATURE}`, `:${FEATURE}`, ',A/B', ',']) {
    const args = command('enabled').map(arg => arg.startsWith('--enable-features=') ? arg + suffix : arg);
    assert.throws(() => verifyCommandLine(args, 'enabled', BROWSER_VERSION));
  }
  assert.throws(() => verifyCommandLine(command('disabled'), 'enabled', BROWSER_VERSION));
  for (const options of [{ args: [] }, { ignoreDefaultArgs: [] }, { executablePath: 'private' }, { channel: 'chrome' }, { headless: false }, { proxy: {} }, { env: {} }]) {
    assert.throws(() => contrastLaunchOptions(options, 'enabled'));
  }
});
test('ordinary options and fixture command are unchanged without exact opt-in', () => {
  const options = { args: ['--ordinary'] };
  assert.equal(portContrastOptions(options, {}), options);
  assert.equal(contrastServerCommand('cargo run -p board-public --example visual-fixtures --locked', {}), 'cargo run -p board-public --example visual-fixtures --locked');
  assert.equal(contrastEnabled({}), false);
  for (const value of ['0', 'true', '', '2']) assert.throws(() => contrastEnabled({ WINDOWS_PORT_CONTRAST: value }));
  assert.throws(() => contrastServerCommand('arbitrary.exe', env));
});
test('entry guard rejects alternate platforms, head/attempt drift, and external launch/debug overrides', () => {
  checkEnvironment(env, 'win32');
  assert.throws(() => checkEnvironment(env, 'linux'));
  for (const patch of [{ GITHUB_RUN_ATTEMPT: '2' }, { THEME_SHARD: '6' }, { CANDIDATE_SHA: 'main' }, { WINDOWS_VISUAL_NETLOG: '0' },
    ...['DEBUG', 'DEBUG_FILE', 'PWDEBUG', 'NODE_OPTIONS', 'PLAYWRIGHT_LEGACY_SCREENSHOT', 'PLAYWRIGHT_BROWSERS_PATH', 'PW_TEST_CONNECT_WS_ENDPOINT', 'WINDOWS_BROWSER_CONTROL'].map(key => ({ [key]: 'PRIVATE' }))]) {
    assert.throws(() => checkEnvironment({ ...env, ...patch }, 'win32'));
  }
  assert.deepEqual(binding(env), run);
  assert.throws(() => armName('default'));
});
test('runner identity accepts only the admitted image, OS build, architecture and Node version', () => {
  const runnerEnv = { ImageOS: runner.image_os, ImageVersion: runner.image_version, WINDOWS_PORT_RUNNER_BUILD: runner.windows_build };
  assert.deepEqual(runnerIdentity(runnerEnv, runner.release, runner.arch, runner.node), runner);
  for (const patch of [{ ImageOS: 'win22' }, { ImageVersion: 'PRIVATE' }, { WINDOWS_PORT_RUNNER_BUILD: '20348.1' }]) {
    assert.throws(() => runnerIdentity({ ...runnerEnv, ...patch }, runner.release, runner.arch, runner.node));
  }
  assert.throws(() => runnerIdentity(runnerEnv, runner.release, 'arm64', runner.node));
});
test('shared fixture manifest is bound to exact head, run, attempt, hash and binary size', () => {
  const manifest = { schema: 1, binding: run, fixture: 'visual-fixtures.exe', fixture_sha256: 'b'.repeat(64), fixture_bytes: 100, rust: '1.94.0' };
  assert.equal(validateManifest(manifest, run), manifest);
  for (const patch of [{ fixture: 'other.exe' }, { fixture_bytes: 0 }, { fixture_sha256: 'bad' }, { rust: '1.95.0' }, { binding: { ...run, run: '111' } }]) {
    assert.throws(() => validateManifest({ ...manifest, ...patch }, run));
  }
});
test('both passing arms remain inconclusive; a failed arm remains a failure', () => {
  const both = qualifyPair(evidence('enabled'), evidence('disabled'), run);
  assert.equal(both.interpretation, 'both-passed-inconclusive');
  const mixed = qualifyPair(evidence('enabled', 1), evidence('disabled'), run);
  assert.equal(mixed.enabled_exit, 1);
  assert.equal(mixed.interpretation, 'matched-observation-only');
  assert.equal(mixed.production_fix_established, false);
  assert.equal(mixed.os_root_cause_established, false);
});
for (const [label, patch] of Object.entries({
  'head': { binding: { ...run, head: 'e'.repeat(40) } }, 'run': { binding: { ...run, run: '999' } },
  'attempt': { binding: { ...run, attempt: 2 } }, 'runner build': { runner: { ...runner, windows_build: '26100.9876' } },
  'runner image': { runner: { ...runner, image_version: '20261002.50.1' } }, 'fixture hash': { fixture_sha256: 'e'.repeat(64) },
  'browser hash': { browser_sha256: 'e'.repeat(64) }, 'order': { order_sha256: 'e'.repeat(64) },
  'missing proof': { browsers: [] }, 'missing command-line proof': { all_command_lines_verified: false },
  'partial execution': { executed: 147 }, 'skipped test': { skipped: 1 }, 'retry': { retries: 1 },
  'feature mismatch': { feature_disabled: false }, 'missing exit': { original: null },
  'masked failure': { original: { kind: 'exit', code: 0 }, failed: 1 }, 'interrupted child': { original: { kind: 'signal', code: 143 } },
})) {
  test(`pair rejects ${label}`, () => assert.throws(() => qualifyPair(evidence('enabled'), { ...evidence('disabled'), ...patch }, run)));
}
test('pair rejects duplicate or altered per-browser proof', () => {
  const bad = evidence('disabled');
  bad.browsers[0].disabled_count--;
  assert.throws(() => qualifyPair(evidence('enabled'), bad, run));
  const duplicate = evidence('disabled');
  duplicate.browsers.push(duplicate.browsers[0]); duplicate.browser_count = duplicate.proof_count = 2;
  assert.throws(() => qualifyPair(evidence('enabled'), duplicate, run));
});
test('workflow retains one build, two unweakened arms and separate always-run qualification', () => {
  const source = readFileSync(new URL('../../.github/workflows/windows-port-randomization.yml', import.meta.url), 'utf8');
  assert.equal((source.match(/cargo \+1\.94\.0 build/g) ?? []).length, 1);
  assert.match(source, /arm: \[enabled, disabled\]/);
  assert.match(source, /fail-fast: false/);
  assert.match(source, /qualify-matched-pair:/);
  assert.doesNotMatch(source, /continue-on-error:|--retries|--workers|--timeout|netsh|Set-ItemProperty/);
});
for (const field of ['fixture_bytes', 'runner.image_os', 'runner.image_version', 'runner.windows_build', 'runner.release', 'runner.arch', 'runner.node']) {
  test(`pair rejects ${field} missing in both arms`, () => {
    const arms = [structuredClone(evidence('enabled')), structuredClone(evidence('disabled'))];
    for (const arm of arms) {
      if (field.startsWith('runner.')) delete arm.runner[field.split('.')[1]];
      else delete arm[field];
    }
    assert.throws(() => qualifyPair(...arms, run));
  });
}
test('Windows environment aliases and casing cannot enable debug, remote or instrumented launch', () => {
  for (const key of ['npm_config_pwdebug', 'npm_package_config_pwdebug', 'npm_config_playwright_browsers_path', 'debug', 'Debug_File', 'pw_instrument_modules', 'SELENIUM_REMOTE_URL']) {
    assert.throws(() => checkEnvironment({ ...env, [key]: 'PRIVATE' }, 'win32'));
  }
});
import { admittedNetlog } from '../../scripts/windows-port-contrast-netlog.mjs';
test('admitted NetLog removes only clientInfo command_line and preserves event chains', () => {
  const parsed = { constants: { clientInfo: { command_line: 'PRIVATE_EXECUTABLE --user-data-dir=PRIVATE_PROFILE', version: BROWSER_VERSION }, logEventTypes: { TCP_CONNECT: 1 } },
    events: [{ source: { id: 5, type: 1 }, type: 1, time: '3', params: { source_dependency: { id: 4, type: 1 }, os_error: 10055 } }], polledData: [{ original: true }] };
  const before = JSON.stringify(parsed);
  const result = admittedNetlog(parsed);
  const copy = JSON.parse(result.body);
  assert.equal(result.command_line_redacted, true);
  assert.equal(JSON.stringify(parsed), before);
  assert.deepEqual(copy.events, parsed.events); assert.deepEqual(copy.polledData, parsed.polledData);
  assert.deepEqual(copy.constants.logEventTypes, parsed.constants.logEventTypes);
  assert.deepEqual(copy.constants.clientInfo, { version: BROWSER_VERSION });
  assert.ok(!result.body.includes('PRIVATE'));
});
test('empty current clientInfo is accepted; unexpected metadata shape is unavailable', () => {
  assert.equal(admittedNetlog({ constants: { clientInfo: {} }, events: [] }).command_line_redacted, false);
  for (const clientInfo of [null, [], { unknown: 'PRIVATE' }, { command_line: {} }, { command_line: 'x'.repeat(131073) }]) {
    assert.throws(() => admittedNetlog({ constants: { clientInfo }, events: [] }));
  }
});
test('failed arm requires admitted event-chain evidence and never a falsely complete copy', () => {
  for (const netlog of [undefined, { schema: 1, status: 'absent', files: 0, command_line_redactions: 0, event_chains_preserved: true },
    { schema: 1, status: 'complete', files: 1, command_line_redactions: 0, event_chains_preserved: false }]) {
    assert.throws(() => qualifyPair({ ...evidence('enabled', 1), netlog }, evidence('disabled'), run));
  }
});
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { copyContrastNetlogs, verifyAdmittedCopies } from '../../scripts/windows-port-contrast-netlog.mjs';
import { sha256 } from '../../scripts/windows-port-contrast-core.mjs';
import PortContrastReporter from '../../scripts/windows-port-contrast-reporter.mjs';
test('admitted copies require exact files, sizes, hashes and narrow metadata after download', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'port-contrast-'));
  try {
    const source = path.join(dir, 'source'), dest = path.join(dir, 'dest');
    mkdirSync(source);
    writeFileSync(path.join(source, 'worker-0-example.json'), JSON.stringify({ constants: { clientInfo: { command_line: 'PRIVATE' } }, events: [] }));
    const status = copyContrastNetlogs(source, dest);
    assert.equal(status.command_line_redactions, 1);
    assert.equal(verifyAdmittedCopies(dest, status), true);
    assert.throws(() => verifyAdmittedCopies(path.join(dir, 'missing'), status));
    assert.throws(() => verifyAdmittedCopies(dest, { ...status, files: 2 }));
    const output = path.join(dest, 'netlog-1.json');
    writeFileSync(output, '{"tampered":true}');
    assert.throws(() => verifyAdmittedCopies(dest, status));
    const malicious = JSON.stringify({ constants: { clientInfo: { command_line: 'PRIVATE' } }, events: [] });
    writeFileSync(output, malicious);
    assert.throws(() => verifyAdmittedCopies(dest, { ...status, captures: [{ ordinal: 1, bytes: Buffer.byteLength(malicious), sha256: sha256(malicious) }] }));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
test('absent NetLogs can never claim downloaded captures', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'port-contrast-'));
  try {
    const absent = copyContrastNetlogs(path.join(dir, 'missing'), path.join(dir, 'dest'));
    assert.equal(absent.status, 'absent');
    assert.equal(verifyAdmittedCopies(path.join(dir, 'dest'), absent), true);
    mkdirSync(path.join(dir, 'dest'));
    writeFileSync(path.join(dir, 'dest', 'unexpected.json'), '{}');
    assert.throws(() => verifyAdmittedCopies(path.join(dir, 'dest'), absent));
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
function fakeWorkload() {
  const tests = Array.from({ length: 148 }, (_, index) => ({ location: { file: path.resolve('tests/themes/owned.spec.js') },
    titlePath: () => ['', 'owned', `case ${index}`], expectedStatus: 'passed', timeout: 30000, annotations: [] }));
  const config = { workers: 1, fullyParallel: false, shard: { current: 7, total: 8 }, maxFailures: 0,
    projects: [{ retries: 0, repeatEach: 1, timeout: 30000 }] };
  return { tests, config, suite: { allTests: () => tests } };
}
test('reporter requires the original full ordered workload and never accepts focused budgets', () => {
  const { tests, config, suite } = fakeWorkload();
  const reporter = new PortContrastReporter();
  reporter.onBegin(config, suite);
  assert.equal(reporter.valid, true);
  for (const test of tests) { reporter.onTestBegin(test, { retry: 0 }); reporter.onTestEnd(test, { retry: 0, workerIndex: 0, status: 'passed' }); }
  assert.equal(reporter.valid, true); assert.equal(reporter.ended.size, 148);
  assert.deepEqual(reporter.started, reporter.expected);
  for (const patch of [{ workers: 2 }, { maxFailures: 1 }, { shard: { current: 6, total: 8 } }, { projects: [{ retries: 1, repeatEach: 1, timeout: 30000 }] }]) {
    const bad = new PortContrastReporter(); bad.onBegin({ ...config, ...patch }, suite); assert.equal(bad.valid, false);
  }
  const short = new PortContrastReporter(); short.onBegin(config, { allTests: () => tests.slice(1) }); assert.equal(short.valid, false);
});
test('reporter rejects duplicate, skipped, retried and interrupted cases', () => {
  for (const result of [{ retry: 1, workerIndex: 0, status: 'passed' }, { retry: 0, workerIndex: 0, status: 'skipped' }, { retry: 0, workerIndex: 0, status: 'interrupted' }]) {
    const { tests, config, suite } = fakeWorkload(), reporter = new PortContrastReporter(); reporter.onBegin(config, suite);
    reporter.onTestBegin(tests[0], result); reporter.onTestEnd(tests[0], result); assert.equal(reporter.valid, false);
  }
  const { tests, config, suite } = fakeWorkload(), reporter = new PortContrastReporter(); reporter.onBegin(config, suite);
  reporter.onTestEnd(tests[0], { retry: 0, workerIndex: 0, status: 'passed' });
  reporter.onTestEnd(tests[0], { retry: 0, workerIndex: 0, status: 'passed' }); assert.equal(reporter.valid, false);
});
import { contrastPreflight } from '../../scripts/windows-port-contrast-core.mjs';
test('hosted Windows Selenium JAR inventory marker is inert without allowing launch overrides', () => {
  for (const key of ['SELENIUM_JAR_PATH', 'selenium_jar_path', 'npm_config_selenium_jar_path']) {
    const input = { ...env, [key]: 'PRIVATE_INVENTORY_PATH' }, before = JSON.stringify(input);
    checkEnvironment(input, 'win32');
    assert.equal(JSON.stringify(input), before);
  }
  for (const key of ['SELENIUM_REMOTE_URL', 'SELENIUM_REMOTE_CAPABILITIES', 'SELENIUM_REMOTE_HEADERS', 'SELENIUM_OTHER', 'npm_config_selenium_remote_url', 'selenium_remote_headers']) {
    assert.throws(() => checkEnvironment({ ...env, SELENIUM_JAR_PATH: 'PRIVATE_INVENTORY_PATH', [key]: 'PRIVATE_OVERRIDE' }, 'win32'));
  }
});
test('preflight failure emits only one fixed stage and preserves admission order', () => {
  const stages = ['environment', 'binding', 'runner', 'fixture', 'browser', 'output'];
  for (const failed of stages) {
    const visited = [], messages = [];
    const checks = Object.fromEntries(stages.map(stage => [stage, () => {
      visited.push(stage);
      if (stage === failed) throw new Error('PRIVATE_PATH PRIVATE_ARGUMENT PRIVATE_ENV');
      return 'PRIVATE_INTERNAL_VALUE';
    }]));
    assert.throws(() => contrastPreflight(checks, message => messages.push(message)), error => error.message === 'Port contrast preflight unavailable');
    assert.deepEqual(visited, stages.slice(0, stages.indexOf(failed) + 1));
    assert.deepEqual(messages, [`Port contrast preflight rejected: ${failed}.`]);
    assert.ok(!messages.join('').includes('PRIVATE'));
  }
});
test('successful preflight returns internal values without logging them; missing stages fail closed', () => {
  const stages = ['environment', 'binding', 'runner', 'fixture', 'browser', 'output'];
  const checks = Object.fromEntries(stages.map(stage => [stage, () => 'PRIVATE_INTERNAL_VALUE']));
  const messages = [];
  const result = contrastPreflight(checks, message => messages.push(message));
  assert.equal(result.fixture, 'PRIVATE_INTERNAL_VALUE'); assert.deepEqual(messages, []);
  delete checks.fixture;
  assert.throws(() => contrastPreflight(checks, message => messages.push(message)));
  assert.deepEqual(messages, ['Port contrast preflight rejected: fixture.']);
});
