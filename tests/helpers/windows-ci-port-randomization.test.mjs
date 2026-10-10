import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { windowsCiLaunchOptions, validateWindowsCiBrowserPins, WINDOWS_CI_PORT_OPT_IN,
  PLAYWRIGHT_VERSION, BROWSER_VERSION, BROWSER_REVISION, CORE_BUNDLE_SHA256,
  DEFAULT_DISABLED, PORT_FEATURE } from './windows-ci-port-randomization.js';
import { visualNetlogPlan } from './visual-netlog.js';
import { publicConfig } from '../../playwright.public-base.js';

const env = { [WINDOWS_CI_PORT_OPT_IN]: '1', CI: 'true', GITHUB_ACTIONS: 'true' };
const pins = () => ({ playwright: PLAYWRIGHT_VERSION, test: PLAYWRIGHT_VERSION, core: PLAYWRIGHT_VERSION,
  bundle: CORE_BUNDLE_SHA256, browsers: ['chromium', 'chromium-headless-shell'].map(name =>
    ({ name, revision: BROWSER_REVISION, browserVersion: BROWSER_VERSION })) });
const read = filename => readFileSync(new URL(`../../${filename}`, import.meta.url), 'utf8');
const rejected = error => error.message === 'Windows CI browser mitigation configuration rejected' && error.cause === undefined;

test('absent opt-in preserves the exact original options on every platform', () => {
  for (const platform of ['win32', 'linux', 'darwin']) {
    for (const options of [{}, { args: ['--original'], timeout: 1234 }]) {
      const before = JSON.stringify(options);
      assert.equal(windowsCiLaunchOptions(options, {}, platform), options);
      assert.equal(JSON.stringify(options), before);
    }
  }
});
test('malformed opt-in and non-Windows or non-CI activation fail closed', () => {
  for (const value of ['', '0', 'true', 'disabled', 1, true, null, 'PRIVATE']) {
    assert.throws(() => windowsCiLaunchOptions({}, { ...env, [WINDOWS_CI_PORT_OPT_IN]: value }, 'win32'), rejected);
  }
  for (const platform of ['linux', 'darwin']) assert.throws(() => windowsCiLaunchOptions({}, env, platform), rejected);
  for (const patch of [{ CI: undefined }, { CI: '1' }, { CI: true }, { GITHUB_ACTIONS: undefined }, { GITHUB_ACTIONS: 'false' }]) {
    assert.throws(() => windowsCiLaunchOptions({}, { ...env, ...patch }, 'win32'), rejected);
  }
});
test('existing launch overrides cannot be silently merged or replaced', () => {
  for (const options of [null, [], { args: [] }, { args: ['--disable-features=PRIVATE'] },
    { ignoreDefaultArgs: true }, { executablePath: 'PRIVATE' }, { channel: 'chrome' },
    { headless: false }, { proxy: {} }, { env: {} }, { timeout: 90000 }]) {
    assert.throws(() => windowsCiLaunchOptions(options, env, 'win32'), rejected);
  }
});
test('launch-affecting environment aliases are rejected without changing owned diagnostics', () => {
  for (const key of ['PWDEBUG', 'pwdebug', 'npm_config_pwdebug', 'npm_package_config_pwdebug',
    'PLAYWRIGHT_LEGACY_SCREENSHOT', 'playwright_legacy_screenshot', 'PW_TEST_CONNECT_WS_ENDPOINT',
    'NODE_OPTIONS', 'node_options', 'npm_config_node_options', 'PW_INSTRUMENT_MODULES',
    'npm_package_config_pw_instrument_modules', 'PLAYWRIGHT_BROWSERS_PATH', 'npm_config_playwright_browsers_path',
    'PLAYWRIGHT_HOST_PLATFORM_OVERRIDE', 'npm_config_playwright_host_platform_override',
    'PW_TEST_CONNECT_HEADERS', 'PW_TEST_REUSE_CONTEXT', 'PWTEST_CLI_IS_UNDER_TEST',
    'SELENIUM_REMOTE_URL', 'SELENIUM_REMOTE_CAPABILITIES', 'SELENIUM_REMOTE_HEADERS', 'SELENIUM_UNKNOWN']) {
    assert.throws(() => windowsCiLaunchOptions({}, { ...env, [key]: 'PRIVATE' }, 'win32'), rejected);
  }
  const diagnosticEnv = { ...env, DEBUG: 'pw:browser',
    WINDOWS_VISUAL_NETLOG: '1', WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS: '1', SELENIUM_JAR_PATH: 'PRIVATE_INVENTORY' };
  const before = JSON.stringify(diagnosticEnv);
  assert.deepEqual(windowsCiLaunchOptions({}, diagnosticEnv, 'win32'), windowsCiLaunchOptions({}, env, 'win32'));
  assert.equal(JSON.stringify(diagnosticEnv), before);
});
test('package, bundle, browser version and revision drift require review', () => {
  validateWindowsCiBrowserPins(pins());
  for (const key of ['playwright', 'test', 'core', 'bundle']) {
    const bad = pins(); bad[key] = 'PRIVATE';
    assert.throws(() => validateWindowsCiBrowserPins(bad), rejected);
  }
  for (const name of ['chromium', 'chromium-headless-shell']) {
    for (const key of ['revision', 'browserVersion']) {
      const bad = pins(); bad.browsers.find(browser => browser.name === name)[key] = 'PRIVATE';
      assert.throws(() => validateWindowsCiBrowserPins(bad), rejected);
    }
    const missing = pins(); missing.browsers = missing.browsers.filter(browser => browser.name !== name);
    assert.throws(() => validateWindowsCiBrowserPins(missing), rejected);
    const duplicate = pins(); duplicate.browsers.push({ ...duplicate.browsers.find(browser => browser.name === name) });
    assert.throws(() => validateWindowsCiBrowserPins(duplicate), rejected);
    const overridden = pins(); overridden.browsers.find(browser => browser.name === name).revisionOverrides = { win64: 'PRIVATE' };
    assert.throws(() => validateWindowsCiBrowserPins(overridden), rejected);
  }
});
test('only the exact original disabled-feature switch is replaced', () => {
  const options = windowsCiLaunchOptions({}, env, 'win32');
  assert.deepEqual(Object.keys(options).sort(), ['args', 'ignoreDefaultArgs']);
  assert.deepEqual(options.ignoreDefaultArgs, [`--disable-features=${DEFAULT_DISABLED.join(',')}`]);
  assert.deepEqual(options.args, [`--disable-features=${[...DEFAULT_DISABLED, PORT_FEATURE].join(',')}`]);
  assert.equal(DEFAULT_DISABLED.length, 16);
  assert.equal(new Set(DEFAULT_DISABLED).size, 16);
  assert.ok(!options.ignoreDefaultArgs.includes(options.args[0]));
  assert.ok(!JSON.stringify(options).includes('--enable-automation'));
});
test('real pinned argument generation preserves all unrelated defaults without automation instrumentation', async () => {
  const require = createRequire(import.meta.url);
  // Argument generation only: never launch Chromium or create a profile.
  const { server } = require('playwright-core/lib/coreBundle');
  const chromium = server.createPlaywright({ sdkLanguage: 'javascript', isServer: true }).chromium;
  const defaults = await chromium.defaultArgs({ headless: true }, false, 'PRIVATE_PROFILE');
  const options = windowsCiLaunchOptions({}, env, 'win32');
  const generated = await chromium.defaultArgs({ ...options, headless: true }, false, 'PRIVATE_PROFILE');
  const actual = generated.filter(arg => !options.ignoreDefaultArgs.includes(arg));
  assert.deepEqual(defaults.filter(arg => arg.startsWith('--disable-features=')), options.ignoreDefaultArgs);
  assert.deepEqual(actual.filter(arg => arg.startsWith('--disable-features=')), options.args);
  assert.deepEqual(actual.filter(arg => !arg.startsWith('--disable-features=')), defaults.filter(arg => !arg.startsWith('--disable-features=')));
  assert.deepEqual(actual.filter(arg => arg.startsWith('--enable-features=')), ['--enable-features=CDPScreenshotNewSurface']);
  assert.equal(actual.includes('--enable-automation'), false);
  assert.equal(actual.includes('--headless'), true);
  assert.equal(actual.includes('--disable-field-trial-config'), true);
  assert.equal(actual.length, defaults.length);
});
test('existing NetLog composition retains the mitigation and its original capture limits', () => {
  const options = windowsCiLaunchOptions({}, env, 'win32');
  const plan = visualNetlogPlan(options, path.resolve('test-results/owned-unit-plan'), 0, 'unit');
  assert.deepEqual(plan.options.ignoreDefaultArgs, options.ignoreDefaultArgs);
  assert.equal(plan.options.args[0], options.args[0]);
  assert.equal(plan.options.args.length, options.args.length + 3);
  assert.equal(plan.options.args.filter(arg => arg.startsWith('--disable-features=')).length, 1);
});
test('ordinary public config has no launchOptions property and retains all workload/server budgets', () => {
  const previous = process.env.VISUAL_FIXTURE_SERVER;
  try {
    const config = publicConfig({ visual: true });
    assert.equal(Object.hasOwn(config.use, 'launchOptions'), false);
    assert.equal(config.use.browserName, 'chromium');
    assert.equal(config.workers, 1); assert.equal(config.retries, 0); assert.equal(config.timeout, 30000);
    assert.equal(config.fullyParallel, false);
    assert.equal(config.webServer.command, 'cargo run -p board-public --example visual-fixtures --locked');
    assert.equal(config.webServer.timeout, 120000); assert.equal(config.webServer.reuseExistingServer, false);
    assert.equal(config.webServer.url, 'http://127.0.0.1:3000/readyz');
  } finally {
    if (previous === undefined) delete process.env.VISUAL_FIXTURE_SERVER;
    else process.env.VISUAL_FIXTURE_SERVER = previous;
  }
});
test('only the five intended required Windows browser steps opt in', () => {
  const ci = read('.github/workflows/ci.yml');
  const expected = [
    'Run deterministic visual fixture without a database service',
    'Run archive desktop and mobile visual fixtures',
    'Run attachment desktop and mobile visual fixtures',
    'Run public empty and error desktop/mobile states',
    'Run referenced theme preferences and desktop/mobile visuals',
  ];
  assert.equal((ci.match(/WINDOWS_CI_DISABLE_TCP_PORT_RANDOMIZATION:/g) ?? []).length, expected.length);
  const steps = ci.split('      - name: ').slice(1);
  assert.deepEqual(steps.filter(step => step.includes(`${WINDOWS_CI_PORT_OPT_IN}:`)).map(step => step.split('\n')[0]), expected);
  for (const name of expected) {
    const step = steps.find(step => step.startsWith(`${name}\n`));
    assert.match(step, /\n        env:\n(?:          [^\n]*\n)*          WINDOWS_CI_DISABLE_TCP_PORT_RANDOMIZATION: '1'\n/);
    assert.doesNotMatch(step, /continue-on-error|--retries|--workers|--timeout|--update-snapshots|exit 0/);
  }
  assert.ok(ci.indexOf(WINDOWS_CI_PORT_OPT_IN) > ci.indexOf('  visual-windows-core:'));
  assert.match(ci, /node --test[^\n]*tests\/helpers\/windows-ci-port-randomization\.test\.mjs/);
  assert.match(ci, /shard: \[1, 2, 3, 4, 5, 6, 7, 8\]/);
});
test('common config reaches all intended suites while diagnostic workflows remain baseline', () => {
  assert.ok(read('playwright.public-base.js').includes('windowsCiLaunchOptions({})'));
  for (const config of ['playwright.config.js', 'playwright.archive-visual.config.js', 'playwright.states.config.js']) {
    assert.ok(read(config).includes("from './playwright.public-base.js'"));
  }
  for (const config of ['playwright.media-visual.config.js', 'playwright.themes.config.js']) {
    assert.ok(read(config).includes("from './playwright.archive-visual.config.js'"));
  }
  for (const workflow of ['windows-browser-control.yml', 'windows-dual-stack-connect.yml',
    'windows-native-transport.yml', 'windows-pooled-transport.yml', 'windows-receive-connect.yml']) {
    assert.ok(!read(`.github/workflows/${workflow}`).includes(WINDOWS_CI_PORT_OPT_IN));
  }
  assert.equal(existsSync(new URL('../../.github/workflows/windows-port-randomization.yml', import.meta.url)), false);
  assert.ok(!read('tests/helpers/windows-ci-port-randomization.js').includes('Browser.getBrowserCommandLine'));
});
