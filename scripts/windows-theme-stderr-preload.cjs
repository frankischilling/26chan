'use strict';

// Intercept complete pw:browser records before Playwright's worker stderr/trace
// interception. An outer stderr filter alone would leave raw records in traces.
const path = require('node:path');
const {
  CARRY_BYTES, EVENT_LIMIT, EVENT_MARKER, TRUNCATED_MARKER, UNAVAILABLE_MARKER, SETUP_REFUSED_MARKER,
  CONTROL_COORDINATOR_READY_MARKER, CONTROL_WORKER_READY_MARKER, recognizeChromiumConnectLine,
} = require('./windows-theme-stderr-filter.cjs');

function installBrowserDebugFilter(debug, emit) {
  if (typeof debug !== 'function' || typeof debug.log !== 'function' || typeof emit !== 'function') {
    throw new Error('Owned browser debug interception unavailable');
  }
  let emitted = 0;
  let reportedTruncation = false;
  const sink = function (...args) {
    // Do not format arbitrary objects or interpolate format strings. The pinned
    // browser logger passes its complete formatted message as one string.
    if (this?.namespace !== 'pw:browser' || args.length !== 1 || typeof args[0] !== 'string' ||
        Buffer.byteLength(args[0], 'utf8') > CARRY_BYTES || !recognizeChromiumConnectLine(args[0])) return;
    let marker;
    if (emitted < EVENT_LIMIT) {
      emitted += 1;
      marker = EVENT_MARKER;
    } else if (!reportedTruncation) {
      reportedTruncation = true;
      marker = TRUNCATED_MARKER;
    }
    if (marker) {
      // Diagnostics cannot replace a browser/test outcome if the output closes.
      try { emit(`${marker}\n`); } catch { }
    }
  };
  debug.log = sink;
  if (debug.log !== sink) throw new Error('Owned browser debug interception unavailable');
  return sink;
}

// Playwright 1.62 forks the pinned workerProcessEntry.js directly, then sets
// TEST_WORKER_INDEX inside WorkerMain. --require runs before that constructor.
// Use the real child entry path at preload time; a coordinator's environment
// (including a forged TEST_WORKER_INDEX) never establishes a worker role.
function isPinnedWorkerEntry(entryScript, playwrightPackagePath, platform) {
  if (typeof entryScript !== 'string' || typeof playwrightPackagePath !== 'string' ||
      !path.isAbsolute(entryScript) || !path.isAbsolute(playwrightPackagePath)) return false;
  const expected = path.join(path.dirname(playwrightPackagePath), 'lib', 'worker', 'workerProcessEntry.js');
  const actual = path.normalize(entryScript);
  const pinned = path.normalize(expected);
  return platform === 'win32'
    ? actual.toLowerCase() === pinned.toLowerCase()
    : actual === pinned;
}

function initializeProbe({ env, platform, version, loadDebug, emit, entryScript, playwrightPackagePath }) {
  let debug;
  try {
    const control = env.WINDOWS_BROWSER_CONTROL === '1' && env.WINDOWS_BROWSER_CONTROL_OWNED === '1';
    const theme = /^[1-8]$/.test(env.THEME_SHARD ?? '') &&
      (!control || env.WINDOWS_BROWSER_CONTROL_SUITE === 'themes');
    const media = control && env.WINDOWS_BROWSER_CONTROL_SUITE === 'media-visual' &&
      env.THEME_SHARD === 'media-visual';
    const worker = isPinnedWorkerEntry(entryScript, playwrightPackagePath, platform);
    if (platform !== 'win32' || env.DEBUG !== 'pw:browser' || env.DEBUG_COLORS !== '0' ||
        env.DEBUG_FILE !== undefined || (!theme && !media) || version !== '1.62.0') {
      throw new Error('Owned browser debug configuration unavailable');
    }
    debug = loadDebug();
    installBrowserDebugFilter(debug, emit);
    // Opt-in control needs positive proof from the coordinator AND its real
    // Playwright test worker. Ordinary theme instrumentation emits nothing new.
    if (control) emit(`${worker ? CONTROL_WORKER_READY_MARKER : CONTROL_COORDINATOR_READY_MARKER}\n`);
    return true;
  } catch {
    // Fallback happens before the CLI imports or starts any tests. Disable only
    // diagnostic output, not browser features, and never rerun started tests.
    env.DEBUG = '';
    delete env.DEBUG_FILE;
    if (!debug) debug = loadDebug();
    if (typeof debug?.disable !== 'function' || typeof debug?.enabled !== 'function') {
      throw new Error('Owned diagnostic setup refused before tests');
    }
    debug.disable();
    if (debug.enabled('pw:browser')) throw new Error('Owned diagnostic setup refused before tests');
    try { emit(`${UNAVAILABLE_MARKER}\n`); } catch { }
    return false;
  }
}

function runGuardedSetup({ env, setup, emit }) {
  try { return setup(); }
  catch {
    env.DEBUG = '';
    delete env.DEBUG_FILE;
    try { emit(`${SETUP_REFUSED_MARKER}\n`); } catch { }
    throw new Error('Owned diagnostic setup refused before tests');
  }
}

module.exports = { installBrowserDebugFilter, initializeProbe, isPinnedWorkerEntry, runGuardedSetup };

if (process.env.WINDOWS_THEME_STDERR_PROBE === '1') {
  runGuardedSetup({
    env: process.env, emit: record => process.stderr.write(record),
    setup: () => {
      const packagePath = require.resolve('playwright-core/package.json');
      const playwrightPackagePath = require.resolve('playwright/package.json');
      // This exact pinned bundle exposes the same debug singleton used by
      // coreBundle's DebugLogger. --require is inherited by Playwright workers.
      return initializeProbe({
        env: process.env, platform: process.platform, version: require(packagePath).version,
        entryScript: process.argv[1], playwrightPackagePath,
        loadDebug: () => require(path.join(path.dirname(packagePath), 'lib/utilsBundle.js')).debug,
        emit: record => process.stderr.write(record),
      });
    },
  });
}
