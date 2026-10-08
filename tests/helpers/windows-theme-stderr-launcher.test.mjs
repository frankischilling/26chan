import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { PassThrough } from 'node:stream';
import { mkdtemp, readFile, rm, symlink, writeFile, mkdir, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { themeLaunchPlan, normalizeChildOutcome, runThemeChild, encodeEvidence, saveEvidence } from '../../scripts/windows-theme-stderr.mjs';
import filter from '../../scripts/windows-theme-stderr-filter.cjs';
import preload from '../../scripts/windows-theme-stderr-preload.cjs';

const { EVENT_MARKER, TRUNCATED_MARKER, UNAVAILABLE_MARKER, SETUP_REFUSED_MARKER, EVENT_LIMIT, CARRY_BYTES, ARTIFACT_BYTES } = filter;
const { installBrowserDebugFilter, initializeProbe, runGuardedSetup } = preload;
const args = ['--shard', '5/8', '--output', 'test-results/windows-themes-5'];
const env = { THEME_SHARD: '5', WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS: '1', WINDOWS_VISUAL_NETLOG: '1', PRESERVED: 'owned' };
const chromiumLine = '[123:456:1008/030405.006:ERROR:net\\socket\\tcp_socket_win.cc:997] connect failed: 10055';
const debugLine = `2026-10-08T03:04:05.006Z pw:browser [pid=123][err] ${chromiumLine}`;

function fakeChild() {
  const child = new EventEmitter(); child.stderr = new PassThrough(); child.signals = [];
  child.kill = signal => { child.signals.push(signal); return true; };
  return child;
}

async function simulate(schedule, options = {}) {
  const child = fakeChild(), signals = new EventEmitter();
  const launch = { command: 'owned-node', args: ['owned-cli', ...args], env, cwd: 'owned-root' };
  let captured;
  const result = await runThemeChild(launch, { signals, spawnChild: (command, passedArgs, opts) => {
    captured = { command, args: passedArgs, options: opts };
    queueMicrotask(() => schedule(child, signals));
    return child;
  }, ...options });
  return { result, captured, child, signals };
}

test('only the fixed owned Windows invocation is admitted and CLI arguments stay unchanged', () => {
  const sourceEnv = { ...env, DEBUG: 'pw:*', DEBUG_FILE: 'PRIVATE_PATH', DEBUG_COLORS: '1' };
  const plan = themeLaunchPlan(args, sourceEnv, 'win32');
  assert.deepEqual(plan.args, ['test', '--config', 'playwright.themes.config.js', ...args]);
  assert.equal(plan.env.DEBUG, 'pw:browser'); assert.equal(plan.env.DEBUG_COLORS, '0');
  assert.equal(plan.env.WINDOWS_THEME_STDERR_PROBE, '1'); assert.equal(plan.env.DEBUG_FILE, undefined);
  assert.equal(plan.env.PRESERVED, 'owned'); assert.equal(sourceEnv.DEBUG_FILE, 'PRIVATE_PATH');
  for (const [values, variables, platform] of [
    [args, env, 'linux'], [args, { ...env, THEME_SHARD: '9' }, 'win32'],
    [args, { ...env, WINDOWS_VISUAL_NETLOG: '0' }, 'win32'],
    [args, { ...env, WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS: '0' }, 'win32'],
    [[...args, '--retries', '1'], env, 'win32'],
    [['--shard', '5/8', '--output', '../PRIVATE_PATH'], env, 'win32'],
    [['--shard', '5/4', ...args.slice(2)], env, 'win32'],
  ]) assert.throws(() => themeLaunchPlan(values, variables, platform));
});

test('early browser debug sink exposes only sanitized records before a simulated trace sees stderr', () => {
  const seenByStderrAndTrace = []; const debug = () => {};
  debug.log = () => { throw new Error('RAW_SINK_MUST_NOT_RUN'); };
  installBrowserDebugFilter(debug, value => seenByStderrAndTrace.push(value));
  const receiver = { namespace: 'pw:browser' };
  for (const value of [
    '2026-10-08T03:04:05.006Z pw:browser <launching> PRIVATE_PATH PRIVATE_COMMAND',
    `${debugLine} PRIVATE_SUFFIX`, debugLine.replace('10055', '10054'),
    '\u001b[31m' + debugLine, debugLine.replace('ERROR:', 'WARNING:'),
    'X'.repeat(CARRY_BYTES + 1) + debugLine,
  ]) debug.log.call(receiver, value);
  debug.log.call(receiver, debugLine, 'PRIVATE_EXTRA');
  debug.log.call({ namespace: 'pw:api' }, debugLine);
  debug.log.call(receiver, { toString() { throw new Error('PRIVATE_OBJECT'); } });
  for (let i = 0; i < EVENT_LIMIT + 10; i++) debug.log.call(receiver, debugLine);
  assert.deepEqual(seenByStderrAndTrace, [...Array(EVENT_LIMIT).fill(EVENT_MARKER + '\n'), TRUNCATED_MARKER + '\n']);
  assert.ok(!JSON.stringify(seenByStderrAndTrace).match(/PRIVATE|pid|tcp_socket|123|456|2026/));
});

test('a closed diagnostic sink cannot replace the original browser/test outcome', () => {
  const debug = () => {}; debug.log = () => {};
  installBrowserDebugFilter(debug, () => { throw new Error('PRIVATE_CLOSED_PIPE'); });
  assert.doesNotThrow(() => debug.log.call({ namespace: 'pw:browser' }, debugLine));
});

test('unavailable interception disables only diagnostic output before the CLI starts once', () => {
  for (const version of ['1.62.0', 'future-version']) {
    const variables = { ...env, DEBUG: 'pw:browser', DEBUG_COLORS: '0' };
    const debug = () => {}; let enabled = true, disables = 0; const emitted = [];
    debug.disable = () => { enabled = false; disables++; };
    debug.enabled = () => enabled;
    // Deliberately unavailable sink. Fallback must not emit any raw logger data.
    const original = structuredClone(variables);
    assert.equal(initializeProbe({ env: variables, platform: 'win32', version,
      loadDebug: () => debug, emit: line => emitted.push(line) }), false);
    assert.equal(variables.DEBUG, ''); assert.equal(disables, 1); assert.equal(enabled, false);
    assert.deepEqual(emitted, [UNAVAILABLE_MARKER + '\n']);
    for (const key of Object.keys(original).filter(key => key !== 'DEBUG')) assert.equal(variables[key], original[key]);
  }
});

test('unsafe fallback refuses setup before tests instead of allowing raw browser logging', () => {
  const variables = { ...env, DEBUG: 'pw:browser', DEBUG_COLORS: '0' };
  assert.throws(() => initializeProbe({ env: variables, platform: 'win32', version: 'future-version',
    loadDebug: () => ({}), emit: () => assert.fail('No unvalidated output') }), /setup refused before tests/);
  assert.equal(variables.DEBUG, '');
});

test('early setup refusal has an explicit sanitized marker and retains its failure status', async () => {
  const emitted = [], variables = { DEBUG: 'pw:browser', DEBUG_FILE: 'PRIVATE_PATH' };
  assert.throws(() => runGuardedSetup({ env: variables, emit: line => emitted.push(line),
    setup: () => { throw new Error('PRIVATE_LOADER_EXCEPTION'); } }), error => {
    assert.equal(error.message, 'Owned diagnostic setup refused before tests'); return true;
  });
  assert.equal(variables.DEBUG, ''); assert.equal(variables.DEBUG_FILE, undefined);
  assert.deepEqual(emitted, [SETUP_REFUSED_MARKER + '\n']);
  const { result } = await simulate(child => { child.stderr.end(emitted[0]); child.emit('close', 1, null); });
  assert.equal(result.outcome.kind, 'diagnostic-setup-refused'); assert.equal(result.outcome.code, 1);
  assert.equal(result.diagnostic.setup_refused, true);
  assert.ok(!encodeEvidence(result).includes('PRIVATE'));
});

test('the pinned real Playwright debug singleton delivers complete records to the early sink without launching a browser', () => {
  const require = createRequire(import.meta.url);
  const packagePath = require.resolve('playwright-core/package.json');
  assert.equal(require(packagePath).version, '1.62.0');
  const { debug } = require(path.join(path.dirname(packagePath), 'lib/utilsBundle.js'));
  const originalSink = debug.log, priorDebug = process.env.DEBUG, priorColors = process.env.DEBUG_COLORS;
  const safeRecords = [];
  try {
    process.env.DEBUG_COLORS = '0'; debug.enable('pw:browser');
    const sink = installBrowserDebugFilter(debug, value => safeRecords.push(value));
    require('playwright-core');
    assert.equal(debug.log, sink, 'Framework loading must not replace the early sink');
    const log = debug('pw:browser');
    log(`<launching> PRIVATE_PATH PRIVATE_COMMAND`);
    log(`[pid=123][err] ${chromiumLine}`);
    assert.deepEqual(safeRecords, [EVENT_MARKER + '\n']);
  } finally {
    debug.log = originalSink; debug.enable(priorDebug ?? '');
    if (priorDebug === undefined) delete process.env.DEBUG; else process.env.DEBUG = priorDebug;
    if (priorColors === undefined) delete process.env.DEBUG_COLORS; else process.env.DEBUG_COLORS = priorColors;
  }
});

test('stdout remains inherited and the child receives unchanged arguments without a shell', async () => {
  const { result, captured } = await simulate(child => {
    child.stderr.write('UNEXPECTED_PRIVATE_STDERR\n');
    child.stderr.write(EVENT_MARKER.slice(0, 20)); child.stderr.end(EVENT_MARKER.slice(20) + '\r\n');
    child.emit('close', 27, null);
  });
  assert.deepEqual(captured.options.stdio, ['inherit', 'inherit', 'pipe']);
  assert.equal(captured.options.shell, false); assert.equal(captured.options.detached, false);
  assert.deepEqual(captured.args, ['owned-cli', ...args]);
  assert.equal(captured.options.env, env);
  assert.deepEqual(result.outcome, { kind: 'exit', code: 27, signal: null });
  assert.equal(result.diagnostic.events.length, 1);
  assert.ok(!encodeEvidence(result).includes('PRIVATE'));
});

test('unexpected or malformed stderr never creates evidence or hides child failure', async () => {
  const { result } = await simulate(child => {
    child.stderr.write('PRIVATE_ERROR\n' + debugLine + '\n' + EVENT_MARKER + ' PRIVATE_TRAILER\n');
    child.stderr.end(); child.emit('close', 17, null);
  });
  assert.equal(result.outcome.code, 17); assert.equal(result.diagnostic.events.length, 0);
  const saved = JSON.parse(encodeEvidence(result));
  assert.equal(saved.absent_match, 'inconclusive'); assert.equal(saved.network_child_stderr_coverage, 'unverified');
  assert.ok(!JSON.stringify(saved).match(/PRIVATE|tcp_socket|pid=/));
});

test('spawn errors, signals and unknown termination all remain nonzero without leaking exceptions', async () => {
  const failed = await runThemeChild({}, { spawnChild() { throw new Error('PRIVATE_SPAWN_PATH'); } });
  assert.equal(failed.outcome.code, 1); assert.ok(!encodeEvidence(failed).includes('PRIVATE'));
  const asynchronous = await simulate(child => { child.emit('error', new Error('PRIVATE_ASYNC_PATH')); child.emit('close', -2, null); });
  assert.equal(asynchronous.result.outcome.kind, 'spawn-error'); assert.equal(asynchronous.result.outcome.code, 1);
  for (const signal of ['SIGTERM', 'SIGINT', 'SIGKILL']) {
    const { result } = await simulate(child => child.emit('close', null, signal));
    assert.equal(result.outcome.signal, signal); assert.ok(result.outcome.code > 0);
  }
  assert.deepEqual(normalizeChildOutcome(null, 'PRIVATE_SIGNAL'), { kind: 'unavailable', code: 1, signal: null });
  assert.equal(normalizeChildOutcome(null, null).code, 1);
});

test('parent cancellation is forwarded and listeners are removed after close', async () => {
  const { result, child, signals } = await simulate((child, signals) => {
    signals.emit('SIGTERM'); child.emit('close', null, 'SIGTERM');
  });
  assert.deepEqual(child.signals, ['SIGTERM']); assert.equal(result.outcome.signal, 'SIGTERM');
  for (const signal of ['SIGTERM', 'SIGINT', 'SIGHUP']) assert.equal(signals.listenerCount(signal), 0);
});

test('late teardown stderr is consumed through close, not lost at exit', async () => {
  const { result } = await simulate(child => {
    child.emit('exit', 23, null);
    child.stderr.end(EVENT_MARKER + '\n');
    child.emit('close', 23, null);
  });
  assert.equal(result.outcome.code, 23); assert.equal(result.diagnostic.events.length, 1);
});

test('stream diagnostic errors preserve the original successful or failed child exit', async () => {
  for (const code of [0, 19]) {
    const { result } = await simulate(child => {
      child.stderr.emit('error', new Error('PRIVATE_STREAM_ERROR')); child.emit('close', code, null);
    });
    assert.equal(result.outcome.code, code); assert.equal(result.stream_error, true);
    assert.ok(!encodeEvidence(result).includes('PRIVATE'));
  }
});

test('sanitized artifact is bounded and never overwrites a file or follows output symlinks', async t => {
  const directory = await realpath(await mkdtemp(path.join(tmpdir(), 'owned-stderr-')));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const { result } = await simulate(child => {
    child.stderr.end((EVENT_MARKER + '\n').repeat(40)); child.emit('close', 1, null);
  });
  const body = encodeEvidence(result);
  assert.ok(Buffer.byteLength(body) <= ARTIFACT_BYTES);
  await saveEvidence(directory, 'test-results/windows-themes-5', body);
  const destination = path.join(directory, 'test-results/windows-themes-5/transport-stderr.json');
  assert.equal(await readFile(destination, 'utf8'), body);
  await assert.rejects(saveEvidence(directory, 'test-results/windows-themes-5', 'PRIVATE_OVERWRITE'));
  await assert.rejects(saveEvidence(directory, '../PRIVATE_ESCAPE', body));
  await assert.rejects(saveEvidence(directory, 'test-results/windows-themes-1', 'X'.repeat(ARTIFACT_BYTES + 1)));
  const other = path.join(directory, 'other'); await mkdir(other);
  const linked = path.join(directory, 'test-results/windows-themes-2');
  try { await symlink(other, linked, process.platform === 'win32' ? 'junction' : 'dir'); }
  catch (error) { if (process.platform === 'win32' && error.code === 'EPERM') return; throw error; }
  await assert.rejects(saveEvidence(directory, 'test-results/windows-themes-2', body));
});

test('launcher retains the package theme command and narrow artifact integration', async () => {
  const root = new URL('../../', import.meta.url);
  const pkg = JSON.parse(await readFile(new URL('package.json', root), 'utf8'));
  assert.equal(pkg.scripts['test:themes'], 'playwright test --config playwright.themes.config.js');
  const source = await readFile(new URL('scripts/windows-theme-stderr.mjs', root), 'utf8');
  assert.ok(source.includes("args: ['--require', preload, cli, ...plan.args]"));
  assert.ok(!source.includes('console.log('), 'The wrapper must not append to child stdout');
  assert.ok(!source.includes('--disable-features')); assert.ok(!source.includes('--enable-features'));
  const worker = await readFile(new URL('node_modules/playwright/lib/runner/index.js', root), 'utf8');
  assert.match(worker, /fork\(this\._entryScript, \{[\s\S]{0,600}\.\.\.process\.env/);
  const ci = await readFile(new URL('.github/workflows/ci.yml', root), 'utf8');
  const job = ci.split('  visual-windows-themes:\n')[1];
  const steps = job.split('      - name: ');
  const uploads = steps.filter(step => step.startsWith('Retain sanitized Windows transport stderr evidence\n'));
  assert.equal(uploads.length, 1);
  const upload = uploads[0];
  assert.match(upload, /\n        if: always\(\)\n/);
  assert.match(upload, /\n          name: windows-transport-stderr-\$\{\{ matrix\.shard \}\}-\$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}\n/);
  assert.match(upload, /\n          path: test-results\/windows-themes-\*\/transport-stderr\.json\n/);
  assert.equal(upload.match(/\n          path:/g)?.length, 1);
  assert.ok(!/netlogs|\.etl|\.log|\.zip|\.png|\*\*|summary\.json/.test(upload));
  assert.match(upload, /\n          retention-days: 3\n/);
  assert.match(upload, /\n          include-hidden-files: false\n/);
  const failure = steps.find(step => step.startsWith('Retain synthetic Windows theme shard failure diagnostics\n'));
  assert.match(failure, /\n        if: failure\(\)\n/);
  assert.match(failure, /test-results\/\*\*\/netlogs\/\*\.json/);
  assert.ok(!failure.includes('transport-stderr.json'));
});
