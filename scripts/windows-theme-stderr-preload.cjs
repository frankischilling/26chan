'use strict';

// Intercept complete pw:browser records before Playwright's worker stderr/trace
// interception. An outer stderr filter alone would leave raw records in traces.
const path = require('node:path');
const {
  CARRY_BYTES, EVENT_LIMIT, EVENT_MARKER, TRUNCATED_MARKER, UNAVAILABLE_MARKER, SETUP_REFUSED_MARKER, recognizeChromiumConnectLine,
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

function initializeProbe({ env, platform, version, loadDebug, emit }) {
  let debug;
  try {
    if (platform !== 'win32' || env.DEBUG !== 'pw:browser' || env.DEBUG_COLORS !== '0' ||
        env.DEBUG_FILE !== undefined || !/^[1-8]$/.test(env.THEME_SHARD ?? '') || version !== '1.62.0') {
      throw new Error('Owned browser debug configuration unavailable');
    }
    debug = loadDebug();
    installBrowserDebugFilter(debug, emit);
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

module.exports = { installBrowserDebugFilter, initializeProbe, runGuardedSetup };

if (process.env.WINDOWS_THEME_STDERR_PROBE === '1') {
  runGuardedSetup({
    env: process.env, emit: record => process.stderr.write(record),
    setup: () => {
      const packagePath = require.resolve('playwright-core/package.json');
      // This exact pinned bundle exposes the same debug singleton used by
      // coreBundle's DebugLogger. --require is inherited by Playwright workers.
      return initializeProbe({
        env: process.env, platform: process.platform, version: require(packagePath).version,
        loadDebug: () => require(path.join(path.dirname(packagePath), 'lib/utilsBundle.js')).debug,
        emit: record => process.stderr.write(record),
      });
    },
  });
}
