import { createHash } from 'node:crypto';
import { readFileSync, statSync } from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';

// Provisional, explicit Windows CI trial. No default change for local runs,
// other platforms, diagnostic workflows or the application itself.
export const WINDOWS_CI_PORT_OPT_IN = 'WINDOWS_CI_DISABLE_TCP_PORT_RANDOMIZATION';
export const PLAYWRIGHT_VERSION = '1.62.0';
export const BROWSER_VERSION = '151.0.7922.34';
export const BROWSER_REVISION = '1234';
export const CORE_BUNDLE_SHA256 = '3258d1cf334c6afc95f22aa9c292436cb976b391e0437f1359c83b84f0cb9d66';
export const PORT_FEATURE = 'TcpPortRandomizationWin';
export const DEFAULT_DISABLED = Object.freeze([
  'AvoidUnnecessaryBeforeUnloadCheckSync', 'BoundaryEventDispatchTracksNodeRemoval',
  'DestroyProfileOnBrowserClose', 'DialMediaRouteProvider', 'GlobalMediaControls',
  'HttpsUpgrades', 'LensOverlay', 'MediaRouter', 'PaintHolding',
  'ThirdPartyStoragePartitioning', 'BlockOriginHeaderModificationOnRedirect',
  'Translate', 'AutoDeElevate', 'OptimizationHints', 'msForceBrowserSignIn',
  'msEdgeUpdateLaunchServicesPreferredVersion',
]);
const require = createRequire(import.meta.url);
const demand = condition => { if (!condition) throw new Error('Windows CI browser mitigation configuration rejected'); };
const digest = value => createHash('sha256').update(value).digest('hex');

export function validateWindowsCiBrowserPins(pins) {
  demand(pins?.playwright === PLAYWRIGHT_VERSION && pins.test === PLAYWRIGHT_VERSION && pins.core === PLAYWRIGHT_VERSION);
  demand(pins.bundle === CORE_BUNDLE_SHA256);
  demand(Array.isArray(pins.browsers));
  for (const name of ['chromium', 'chromium-headless-shell']) {
    const matches = pins.browsers.filter(browser => browser?.name === name);
    demand(matches.length === 1 && matches[0].revision === BROWSER_REVISION && matches[0].browserVersion === BROWSER_VERSION);
    demand(matches[0].revisionOverrides === undefined);
  }
}
function installedPins() {
  const packageRoot = path.dirname(require.resolve('playwright-core/package.json'));
  const read = (file, maximum) => {
    const info = statSync(file);
    demand(info.isFile() && info.size > 0 && info.size <= maximum);
    return readFileSync(file);
  };
  return {
    playwright: require('playwright/package.json').version,
    test: require('@playwright/test/package.json').version,
    core: require('playwright-core/package.json').version,
    bundle: digest(read(path.join(packageRoot, 'lib/coreBundle.js'), 8388608)),
    browsers: JSON.parse(read(path.join(packageRoot, 'browsers.json'), 65536)).browsers,
  };
}

export function windowsCiLaunchOptions(options, env = process.env, platform = process.platform) {
  // Preserve the exact caller object and avoid reading installed pins when off.
  if (env[WINDOWS_CI_PORT_OPT_IN] === undefined) return options;
  try {
    demand(env[WINDOWS_CI_PORT_OPT_IN] === '1' && platform === 'win32' && env.CI === 'true' && env.GITHUB_ACTIONS === 'true');
    demand(options && typeof options === 'object' && !Array.isArray(options) && Object.keys(options).length === 0);
    for (const key of Object.keys(env)) {
      const normalized = key.replace(/^npm_(?:config|package_config)_/i, '').toUpperCase();
      // Existing owned stderr logging and NetLog remain unchanged. These
      // separate overrides can bypass launch or alter the pinned defaults.
      if (/^(?:PWDEBUG|PW_TEST_CONNECT|PW_TEST_REUSE_CONTEXT|PWTEST_)/.test(normalized) ||
          ['NODE_OPTIONS', 'PW_INSTRUMENT_MODULES', 'PLAYWRIGHT_BROWSERS_PATH',
            'PLAYWRIGHT_HOST_PLATFORM_OVERRIDE', 'PLAYWRIGHT_LEGACY_SCREENSHOT'].includes(normalized) ||
          (normalized.startsWith('SELENIUM_') && normalized !== 'SELENIUM_JAR_PATH')) {
        demand(env[key] === undefined || env[key] === '');
      }
    }
    validateWindowsCiBrowserPins(installedPins());
    // Playwright filters the entire final argv, including caller additions.
    // Remove only the original disable switch; keep all its features in the
    // distinct replacement. The original enable switch and other args survive.
    return {
      ignoreDefaultArgs: [`--disable-features=${DEFAULT_DISABLED.join(',')}`],
      args: [`--disable-features=${[...DEFAULT_DISABLED, PORT_FEATURE].join(',')}`],
    };
  } catch { throw new Error('Windows CI browser mitigation configuration rejected'); }
}
