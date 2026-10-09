import test from 'node:test';
import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { mkdir, mkdtemp, realpath, readFile, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { browserControlEnabled, createBrowserControlTrigger, CONTROL_TRIGGER } from './browser-control-trigger.js';
import { CONTROL_SCHEMA, controlIdentity, diagnosticPlan, createTriggerCollector, inspectControlNetlog, TARGET, bounded, encodeSummary } from '../../scripts/windows-browser-control-core.mjs';
import { themeLaunchPlan } from '../../scripts/windows-theme-stderr.mjs';
import { coordinate, saveControlSummary } from '../../scripts/windows-browser-control.mjs';
import { runControl } from '../../scripts/windows-browser-control-child.mjs';
import { qualifySummary, checkEvidence } from '../../scripts/check-windows-browser-control.mjs';
const env = { WINDOWS_BROWSER_CONTROL: '1', WINDOWS_BROWSER_CONTROL_OWNED: '1', VISUAL_FIXTURE_SERVER: '1', WINDOWS_VISUAL_NETLOG: '1', WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS: '1', THEME_SHARD: '7' };
const outcome = { kind: 'exit', code: 19, signal: null };
const request = (error = 'net::ERR_NO_BUFFER_SPACE', url = TARGET) => ({ failure: () => ({ errorText: error }), url: () => url });
const tick = () => new Promise(resolve => setImmediate(resolve));

function netlog() {
  return { constants: { logSourceType: { URL_REQUEST: 1, HTTP_STREAM_JOB: 2, SOCKET: 3 }, logEventTypes: { URL_REQUEST_START_JOB: 1, HTTP_STREAM_REQUEST_BOUND_TO_JOB: 2, TCP_CONNECT_ATTEMPT: 3, SOCKET_POOL_BOUND_TO_SOCKET: 4 }, logEventPhase: { PHASE_BEGIN: 1, PHASE_END: 2, PHASE_NONE: 0 } }, events: [
    { type: 1, phase: 1, time: '100', source: { id: 1, type: 1 }, params: { url: TARGET } },
    { type: 2, phase: 0, time: '104', source: { id: 1, type: 1 }, params: { source_dependency: { id: 2, type: 2 } } },
    { type: 3, phase: 1, time: '102', source: { id: 3, type: 3 }, params: { address: '127.0.0.1:3000' } },
    { type: 3, phase: 2, time: '103', source: { id: 3, type: 3 } },
    { type: 4, phase: 0, time: '104', source: { id: 2, type: 2 }, params: { source_dependency: { id: 3, type: 3 } } },
  ] };
}
function fakeControl({ navigation = 'http-200', failSetup = false, silent = false } = {}) {
  const child = new EventEmitter(); child.connected = true; child.commands = [];
  child.disconnect = () => { child.connected = false; }; child.unref = () => {};
  child.send = (message, callback) => {
    child.commands.push(message.type); callback?.();
    if (message.type === 'navigate') {
      child.emit('message', { type: 'started' });
      child.emit('message', { type: 'completed', navigation, status: navigation === 'http-200' ? 200 : null });
    }
    if (message.type === 'stop') queueMicrotask(() => {
      const triggered = child.commands.includes('navigate');
      child.emit('message', { type: 'finished', closed: true, netlog: triggered ? 'accepted' : 'discarded-success', tcp: triggered ? inspectControlNetlog(netlog()) : null });
      child.emit('exit', 0); child.emit('close', 0);
    });
  };
  if (!silent) queueMicrotask(() => child.emit('message', failSetup ? { type: 'unavailable' } : { type: 'ready', package_version: '1.62.0' }));
  return child;
}
function ownership(summary) { return { schema: CONTROL_SCHEMA, shard: summary.shard, phase: 'finished', hard_deadline: false, forced_cleanup: false, tree_exited: true, cleanup_verified: true, original_coordinator_exit: summary.original.code }; }
async function simulate({ shard = '7', trigger = true, controlOptions } = {}) {
  let child, time = 0;
  const summary = await coordinate({ shard, now: () => ++time, startControl: () => child = fakeControl(controlOptions), runOriginal: async fire => {
    if (trigger) { fire(); fire(); }
    return { outcome, evidenceSaved: true };
  } });
  return { child, summary };
}

test('all exact opt-in guards are required; original shard arguments remain unchanged', () => {
  assert.equal(browserControlEnabled('win32', env), true);
  assert.deepEqual(diagnosticPlan(env, 'win32').args, ['--shard', '7/8', '--output', 'test-results/windows-themes-7']);
  for (const platform of ['linux', 'darwin']) { assert.equal(browserControlEnabled(platform, env), false); assert.throws(() => diagnosticPlan(env, platform)); }
  for (const key of Object.keys(env)) for (const value of ['', undefined, 'true', '0']) {
    const bad = { ...env, [key]: value };
    assert.equal(browserControlEnabled('win32', bad), false); assert.throws(() => diagnosticPlan(bad, 'win32'));
  }
});
test('trigger accepts only exact owned failures and emits no request data', () => {
  const records = [], trigger = createBrowserControlTrigger(value => records.push(value));
  for (const item of [request('net::ERR_CONNECTION_FAILED'), request(undefined, 'https://private.invalid/'), request(undefined, 'http://user:secret@127.0.0.1:3000/'), request(undefined, 'http://127.0.0.1:9999/'), request(undefined, 'not a URL')]) trigger(item);
  assert.deepEqual(records, []);
  trigger(request(undefined, TARGET + '?PRIVATE=secret')); trigger(request());
  assert.deepEqual(records, [CONTROL_TRIGGER + '\n']);
  assert.doesNotThrow(() => createBrowserControlTrigger(() => { throw new Error('closed'); })(request()));
});
test('marker parser is bounded, one-shot, and rejects contaminated or unterminated records', () => {
  let calls = 0; const collect = createTriggerCollector(() => calls++);
  for (const text of ['PRIVATE' + CONTROL_TRIGGER + '\n', 'x'.repeat(10000) + CONTROL_TRIGGER + '\n', '\x1b' + CONTROL_TRIGGER + '\n', CONTROL_TRIGGER + '\r\n']) collect(Buffer.from(text));
  collect(Buffer.from(CONTROL_TRIGGER.slice(0, 7))); collect(Buffer.from(CONTROL_TRIGGER.slice(7) + '\n' + CONTROL_TRIGGER + '\n'));
  assert.equal(calls, 1);
});
test('fresh TCP proof requires exact target, dependency, endpoint and same-log temporal order', () => {
  assert.equal(inspectControlNetlog(netlog()).fresh_tcp, 'observed');
  for (const mutate of [log => log.events.splice(1, 1), log => log.events[0].params.url += '?other', log => log.events[2].params.address = '127.0.0.1:3004', log => log.events[2].time = '99', log => log.events[2].phase = 2, log => log.events[2].source.id = 5, log => log.events.push({ ...log.events[0] })]) {
    const log = netlog(); mutate(log); assert.equal(inspectControlNetlog(log).fresh_tcp, 'unverified');
  }
  const log = netlog(); log.events.splice(4, 0, { ...log.events[2], time: '103' });
  assert.equal(inspectControlNetlog(log).attempts, 2);
  assert.equal(inspectControlNetlog({}).fresh_tcp, 'unverified');
});
test('coordinator dispatches once, retains original failure and records one parent clock', async () => {
  const { child, summary } = await simulate();
  assert.deepEqual(child.commands, ['navigate', 'stop']);
  assert.deepEqual(summary.original, outcome);
  assert.equal(summary.conclusion, 'control-connected-after-observed-failure');
  assert.equal(summary.trigger_to_completion_ms, summary.completion_receipt_ms - summary.trigger_receipt_ms);
  assert.equal(qualifySummary(summary, ownership(summary), '7'), 'triggered');
  for (const key of ['forced_cleanup', 'hard_deadline']) assert.equal(qualifySummary(summary, { ...ownership(summary), [key]: true }, '7'), 'unverified-cleanup');
});
test('no trigger is valid inconclusive evidence and does not navigate', async () => {
  const { child, summary } = await simulate({ trigger: false });
  assert.deepEqual(child.commands, ['stop']); assert.equal(summary.conclusion, 'inconclusive');
  assert.equal(qualifySummary(summary, ownership(summary), '7'), 'no-trigger');
});
test('unavailable setup and control failure cannot replace the original exit', async () => {
  for (const controlOptions of [{ failSetup: true }, { navigation: 'failed-or-timeout' }]) {
    const { summary } = await simulate({ controlOptions }); assert.deepEqual(summary.original, outcome);
    assert.equal(summary.conclusion, 'inconclusive');
    if (controlOptions.failSetup) assert.equal(qualifySummary(summary, ownership(summary), '7'), 'invalid-summary');
  }
});
test('bounded rejected operations settle without leaking their error or hanging', async () => {
  assert.equal(await bounded(Promise.reject(new Error('PRIVATE')), 50, 'unavailable'), 'unavailable');
  assert.equal(await bounded(new Promise(() => {}), 1, 'timeout'), 'timeout');
  assert.throws(() => encodeSummary({ private: 'x'.repeat(8192) }));
});
test('separate control launches without inherited options and makes one exact navigation', async () => {
  const root = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-child-')));
  try {
    const messages = new EventEmitter(); messages.disconnect = () => {};
    const sent = [], calls = []; let options;
    await runControl({ root, version: '1.62.0', messages, send: value => sent.push(value), launch: async value => {
      options = value;
      return { newContext: async () => ({ newPage: async () => ({ on: () => {}, goto: async (url, opts) => {
        calls.push({ url, opts }); return { status: () => 200, url: () => TARGET };
      } }) }), close: async () => {
        const filename = options.args.find(arg => arg.startsWith('--log-net-log=')).slice('--log-net-log='.length);
        await writeFile(filename, JSON.stringify(netlog()));
      } };
    } });
    assert.deepEqual(calls, []); assert.equal(sent[0].type, 'ready');
    assert.equal(options.args.length, 3); assert.equal(options.headless, true);
    assert.ok(!options.args.join(' ').includes('windows-themes-7'));
    messages.emit('message', { type: 'navigate' }); messages.emit('message', { type: 'navigate' });
    await tick(); messages.emit('message', { type: 'stop' });
    for (let i = 0; i < 50 && !sent.some(value => value.type === 'finished'); i++) await new Promise(resolve => setTimeout(resolve, 5));
    assert.equal(calls.length, 1); assert.equal(calls[0].url, TARGET); assert.equal(calls[0].opts.timeout, 5000);
    assert.equal(sent.at(-1).netlog, 'accepted'); assert.equal(sent.at(-1).closed, true);
    assert.equal(JSON.parse(await readFile(path.join(root, 'control-netlog.json'), 'utf8')).events.length, 5);
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('admitted files are bounded, no-overwrite, and revalidated by the qualification check', async () => {
  const temporary = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-evidence-')));
  const root = path.join(temporary, 'test-results', 'windows-browser-control-7');
  await mkdir(root, { recursive: true });
  try {
    const { summary } = await simulate();
    await saveControlSummary(root, summary); await assert.rejects(saveControlSummary(root, summary));
    await writeFile(path.join(root, 'ownership.json'), JSON.stringify(ownership(summary)));
    await writeFile(path.join(root, 'control-netlog.json'), JSON.stringify(netlog()));
    assert.equal(await checkEvidence(root, '7'), 'triggered');
    await writeFile(path.join(root, 'control-netlog.json'), '{}'); await assert.rejects(checkEvidence(root, '7'));
    await writeFile(path.join(root, 'summary.json'), 'x'.repeat(8193)); await assert.rejects(checkEvidence(root, '7'));
  } finally { await rm(temporary, { recursive: true, force: true }); }
});
test('source guards retain job containment, no raw files, original status and finite deadlines', async () => {
  const ps = await readFile(new URL('../../scripts/windows-browser-control.ps1', import.meta.url), 'utf8');
  assert.match(ps, /DualStackOwnedProcess\]::Start/); assert.match(ps, /'NUL', 'NUL'/);
  assert.match(ps, /1080000/); assert.match(ps, /\$owned\.Kill\(\)/); assert.match(ps, /exit \$exitCode/);
  const hook = await readFile(new URL('./visual-diagnostics.js', import.meta.url), 'utf8');
  assert.match(hook, /writeSync\(2, line\)/); assert.match(hook, /context\.off\('requestfailed', trigger\)/);
  const child = await readFile(new URL('../../scripts/windows-browser-control-child.mjs', import.meta.url), 'utf8');
  assert.doesNotMatch(child, /newCDPSession|ignoreDefaultArgs|route\(|setDefaultTimeout|\.retry\(/);
});

test('late launch after stop is closed and cannot announce readiness', async () => {
  const root = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-late-')));
  try {
    const messages = new EventEmitter(); messages.disconnect = () => {};
    const sent = []; let resolveLaunch, launched = false, closes = 0;
    const running = runControl({ root, version: '1.62.0', messages, send: message => sent.push(message), launch: () => {
      launched = true; return new Promise(resolve => { resolveLaunch = resolve; });
    } });
    while (!launched) await tick();
    messages.emit('message', { type: 'stop' }); await tick();
    resolveLaunch({ close: async () => { closes++; }, newContext: () => assert.fail('No late context') });
    await running; assert.equal(closes, 1); assert.ok(!sent.some(message => message.type === 'ready'));
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('context/page setup errors close the owned browser without raw errors', async () => {
  for (const phase of ['context', 'page']) {
    const root = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-setup-')));
    try {
      const messages = new EventEmitter(); messages.disconnect = () => {};
      const sent = []; let closes = 0;
      await runControl({ root, version: '1.62.0', messages, send: message => sent.push(message), launch: async () => ({
        newContext: async () => {
          if (phase === 'context') throw new Error('PRIVATE');
          return { newPage: async () => { throw new Error('PRIVATE'); } };
        }, close: async () => { closes++; },
      }) });
      assert.equal(closes, 1); assert.ok(!sent.some(message => message.type === 'ready'));
      assert.ok(!JSON.stringify(sent).includes('PRIVATE'));
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});
test('IPC disconnect during navigation still closes and settles the one operation', async () => {
  const root = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-disconnect-')));
  try {
    const messages = new EventEmitter(); messages.disconnect = () => {};
    const sent = []; let resolveNavigation, closes = 0, calls = 0;
    await runControl({ root, version: '1.62.0', messages, send: message => sent.push(message), launch: async () => ({
      newContext: async () => ({ newPage: async () => ({ on: () => {}, goto: () => {
        calls++; return new Promise(resolve => { resolveNavigation = resolve; });
      } }) }), close: async () => { closes++; },
    }) });
    messages.emit('message', { type: 'navigate' }); messages.emit('disconnect');
    resolveNavigation({ status: () => 200, url: () => TARGET });
    for (let i = 0; i < 50 && !sent.some(message => message.type === 'finished'); i++) await new Promise(resolve => setTimeout(resolve, 5));
    assert.equal(calls, 1); assert.equal(closes, 1); assert.equal(messages.listenerCount('message'), 0);
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('reversed or unrelated graph dependencies do not qualify a TCP attempt', () => {
  const reversed = netlog(); reversed.events[1] = { ...reversed.events[1], source: { id: 2, type: 2 }, params: { source_dependency: { id: 1, type: 1 } } };
  assert.equal(inspectControlNetlog(reversed).fresh_tcp, 'unverified');
  const unrelated = netlog(); unrelated.events[1].params.source_dependency = { id: 3, type: 3 };
  assert.equal(inspectControlNetlog(unrelated).fresh_tcp, 'unverified');
});

test('TCP BEGIN alone and failed ENDs never support successful-control inference', () => {
  assert.equal(inspectControlNetlog(netlog()).successful_tcp, 'observed');
  for (const mutate of [
    log => log.events.splice(3, 1),
    log => { log.events[3].params = { os_error: 10055 }; },
    log => { log.events[3].params = { os_error: 0 }; },
    log => { log.events[3].params = { net_error: 0 }; },
    log => { log.events[3].params = { unknown: true }; },
    log => { log.events[3].time = '105'; },
    log => { log.events[3].source.id = 99; },
    log => { log.events.splice(3, 0, { ...log.events[2], params: { address: '127.0.0.1:3004' } }); },
  ]) {
    const log = netlog(); mutate(log); assert.equal(inspectControlNetlog(log).successful_tcp, 'unverified');
  }
  const earlier = netlog(); earlier.events[2].time = '100'; const attempt = earlier.events.splice(2, 1)[0]; earlier.events.unshift(attempt);
  assert.equal(inspectControlNetlog(earlier).fresh_tcp, 'unverified');
});
test('late ready cannot qualify a startup that already timed out', async () => {
  let child;
  const summary = await coordinate({ shard: '7', startupMs: 1, startControl: () => {
    child = fakeControl({ silent: true });
    return child;
  }, runOriginal: async trigger => {
    child.emit('message', { type: 'ready', package_version: '1.62.0' });
    trigger(); return { outcome, evidenceSaved: true };
  } });
  assert.equal(summary.control, 'unavailable'); assert.ok(!child.commands.includes('navigate'));
  assert.equal(qualifySummary(summary, ownership(summary), '7'), 'invalid-summary');
});
test('same-error failures on every off-origin variant are ignored', () => {
  const records = [], trigger = createBrowserControlTrigger(record => records.push(record));
  for (const url of ['http://localhost:3000/readyz', 'http://127.0.0.1:3004/readyz', 'http://[::1]:3000/readyz', 'https://127.0.0.1:3000/readyz', 'http://127.0.0.1:3000.evil.invalid/readyz', 'about:blank', 'malformed']) trigger(request(undefined, url));
  assert.deepEqual(records, []); trigger(request()); assert.equal(records.length, 1);
});

test('real Playwright worker forwards the synchronous trigger through stderr without browser launch', async () => {
  const { createRequire } = await import('node:module');
  const { execFile } = await import('node:child_process');
  const { promisify } = await import('node:util');
  const require = createRequire(import.meta.url);
  const root = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-worker-')));
  try {
    const testPackage = require.resolve('@playwright/test');
    await writeFile(path.join(root, 'marker.spec.cjs'), `const { test } = require(${JSON.stringify(testPackage)});\nconst { writeSync } = require('node:fs');\ntest('owned marker transport', async () => writeSync(2, ${JSON.stringify(CONTROL_TRIGGER + '\n')}));\n`);
    const config = path.join(root, 'playwright.config.cjs');
    await writeFile(config, `module.exports = { testDir: ${JSON.stringify(root)}, outputDir: ${JSON.stringify(path.join(root, 'output'))}, reporter: 'dot', workers: 1, retries: 0, timeout: 5000 };\n`);
    const { stdout, stderr } = await promisify(execFile)(process.execPath, [require.resolve('@playwright/test/cli'), 'test', '--config', config],
      { cwd: root, env: { ...process.env, DEBUG: '', FORCE_COLOR: '0' }, timeout: 15000, maxBuffer: 65536 });
    let fired = 0; createTriggerCollector(() => fired++)(Buffer.from(stderr));
    assert.equal(fired, 1); assert.ok(!stdout.includes(CONTROL_TRIGGER));
  } finally { await rm(root, { recursive: true, force: true }); }
});

test('stop during pending context/page setup closes without a late ready event', async () => {
  for (const phase of ['context', 'page']) {
    const root = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-pending-')));
    try {
      const messages = new EventEmitter(); messages.disconnect = () => {};
      const sent = []; let release, pending = false, closes = 0;
      const page = { on() {}, goto: () => assert.fail('No navigation') };
      const context = { newPage: async () => {
        if (phase === 'context') return page;
        pending = true; return new Promise(resolve => { release = () => resolve(page); });
      } };
      const running = runControl({ root, version: '1.62.0', messages, send: message => sent.push(message), launch: async () => ({
        newContext: async () => {
          if (phase === 'page') return context;
          pending = true; return new Promise(resolve => { release = () => resolve(context); });
        }, close: async () => { closes++; release?.(); },
      }) });
      while (!pending) await tick();
      messages.emit('message', { type: 'stop' }); await running;
      assert.ok(closes >= 1); assert.ok(!sent.some(message => message.type === 'ready'));
    } finally { await rm(root, { recursive: true, force: true }); }
  }
});
test('unknown source types or phase schema cannot invent a successful socket proof', () => {
  for (const mutate of [
    log => { delete log.constants.logSourceType; },
    log => { delete log.constants.logEventPhase; },
    log => { log.constants.logSourceType.SOCKET = 99; },
    log => { log.events[1].params.source_dependency.type = 99; },
    log => { log.events[4].params.source_dependency.type = 99; },
  ]) { const log = netlog(); mutate(log); assert.equal(inspectControlNetlog(log).successful_tcp, 'unverified'); }
});

test('signaled original outcome retains its signal and normalized fatal code', async () => {
  const signaled = { kind: 'signal', code: 143, signal: 'SIGTERM' };
  const summary = await coordinate({ shard: '7',
    startControl: () => fakeControl(),
    runOriginal: async () => ({ outcome: signaled, evidenceSaved: true }),
  });
  assert.deepEqual(summary.original, signaled);
  assert.notEqual(summary.original.code, 0);
  assert.equal(summary.trigger, 'not-observed');
  assert.equal(summary.conclusion, 'inconclusive');
});

test('owned cleanup grace shares one five-second budget and still requires actual zero processes', async () => {
  const cleanup = await readFile(new URL('../../scripts/windows-browser-control-cleanup.ps1', import.meta.url), 'utf8');
  const wrapper = await readFile(new URL('../../scripts/windows-browser-control.ps1', import.meta.url), 'utf8');
  const controls = await readFile(new URL('../../scripts/windows-browser-control-cleanup.test.ps1', import.meta.url), 'utf8');
  assert.match(cleanup, /\$rootHasExited = & \$RootExited\s+\$remaining = \[Math\]::Max\(0, 5000 - \(& \$ElapsedMilliseconds\)\)\s+if \(\$rootHasExited -and \$remaining -gt 0\)/);
  assert.match(cleanup, /\$grantedGrace = \[int\]\[Math\]::Min\(250, \[Math\]::Max\(0, 5000 - \$graceStarted\)\)/);
  assert.match(cleanup, /\$graceDeadline = \$graceStarted \+ \$grantedGrace/);
  assert.equal((cleanup.match(/5000 - \(& \$ElapsedMilliseconds\)/g) ?? []).length, 2);
  assert.match(cleanup, /if \(-not \(& \$TreeExited\)\) \{\s+\$result.forced_cleanup = \$true\s+& \$Kill/);
  assert.match(cleanup, /\$result.tree_exited = \[bool\]\(& \$TreeExited\)/);
  assert.match(cleanup, /\$graceOnTime = \$null -eq \$graceDeadline -or \$finishedAt -le \$graceDeadline/);
  assert.match(cleanup, /\$result.cleanup_verified = \$result.tree_exited -and \$graceOnTime -and \$finishedAt -lt 5000/);
  assert.ok(cleanup.indexOf('$finishedAt = & $ElapsedMilliseconds') > cleanup.indexOf('$result.tree_exited = [bool](& $TreeExited)'));
  assert.match(wrapper, /\$cleanupWatch = \[Diagnostics.Stopwatch\]::StartNew\(\)/);
  assert.match(wrapper, /-WaitForExit \{ param\(\$milliseconds\) \$owned.WaitForExit\(\$milliseconds\) \} -Kill \{ \$owned.Kill\(\) \}/);
  assert.match(wrapper, /-ElapsedMilliseconds \{ \$cleanupWatch.ElapsedMilliseconds \} -State \$state/);
  assert.doesNotMatch(cleanup + wrapper, /Stop-Process|taskkill|\.Kill\([^)]*Id/);
  for (const name of ['already-empty', 'settled', 'persistent', 'live-root', 'misleading-wait', 'expired', 'near-deadline', 'survivor', 'late-zero', 'slow-grace-zero', 'slow-zero-read', 'unavailable']) assert.ok(controls.includes(`'${name}'`));
});


test('all eight shard identities retain the exact original launch plan and independent output', async () => {
  const outputs = new Set();
  for (let n = 1; n <= 8; n++) {
    const shard = String(n), current = { ...env, THEME_SHARD: shard };
    assert.equal(browserControlEnabled('win32', current), true);
    const plan = diagnosticPlan(current, 'win32');
    assert.equal(plan.shard, shard);
    assert.deepEqual(plan.args, ['--shard', `${shard}/8`, '--output', `test-results/windows-themes-${shard}`]);
    const original = themeLaunchPlan(plan.args, current, 'win32');
    const other = shard === '8' ? '1' : String(n + 1);
    assert.throws(() => themeLaunchPlan(plan.args, { ...current, THEME_SHARD: other }, 'win32'));
    assert.throws(() => themeLaunchPlan(['--shard', `${shard}/8`, '--output', `test-results/windows-themes-${other}`], current, 'win32'));
    assert.deepEqual(original.args, ['test', '--config', 'playwright.themes.config.js', ...plan.args]);
    assert.equal(plan.output, `test-results/windows-browser-control-${shard}`);
    assert.notEqual(plan.output, original.output);
    outputs.add(plan.output);
    for (const trigger of [true, false]) {
      const { summary, child } = await simulate({ shard, trigger });
      assert.equal(summary.schema, CONTROL_SCHEMA); assert.equal(summary.shard, shard);
      assert.equal(qualifySummary(summary, ownership(summary), shard), trigger ? 'triggered' : 'no-trigger');
      assert.deepEqual(summary.original, outcome);
      assert.deepEqual(child.commands, trigger ? ['navigate', 'stop'] : ['stop']);
    }
  }
  assert.equal(outputs.size, 8);
});

test('invalid shard values cannot enable a trigger, select a path, or start a coordinator', async () => {
  for (const shard of [undefined, null, 7, {}, [], '0', '9', '10', '-1', '07', '7/8', ' 7', '7 ', '7\n', '7\r\n', '../7', '７', '7/../8', '']) {
    const current = { ...env, THEME_SHARD: shard };
    assert.equal(browserControlEnabled('win32', current), false);
    assert.throws(() => controlIdentity(shard));
    assert.throws(() => diagnosticPlan(current, 'win32'));
    let started = false;
    await assert.rejects(coordinate({ shard, startControl: () => { started = true; }, runOriginal: () => { started = true; } }));
    assert.equal(started, false);
    const { summary } = await simulate();
    assert.equal(qualifySummary(summary, ownership(summary), shard), 'invalid-shard');
  }
});

test('qualification binds both schema versions and shard identities to the selected path', async () => {
  const temporary = await realpath(await mkdtemp(path.join(os.tmpdir(), 'browser-control-binding-')));
  try {
    const { summary } = await simulate({ shard: '3', trigger: false });
    const owner = ownership(summary);
    const root = path.join(temporary, 'test-results', 'windows-browser-control-3');
    await mkdir(root, { recursive: true });
    await saveControlSummary(root, summary);
    await writeFile(path.join(root, 'ownership.json'), JSON.stringify(owner));
    assert.equal(await checkEvidence(root, '3'), 'no-trigger');
    for (const shard of ['1', '7', '8', '', '3\n']) await assert.rejects(checkEvidence(root, shard));
    for (const value of [undefined, 3, '7', '03', '3\n']) {
      assert.equal(qualifySummary({ ...summary, shard: value }, owner, '3'), 'shard-mismatch');
      assert.equal(qualifySummary(summary, { ...owner, shard: value }, '3'), 'shard-mismatch');
    }
    for (const schema of [undefined, 1, 3, '2']) {
      assert.equal(qualifySummary({ ...summary, schema }, owner, '3'), 'invalid-summary');
      assert.equal(qualifySummary(summary, { ...owner, schema }, '3'), 'unverified-cleanup');
    }
    for (const other of [path.join(temporary, 'windows-browser-control-3'), path.join(temporary, 'test-results', 'windows-browser-control-7')]) {
      await mkdir(other, { recursive: true });
      await saveControlSummary(other, summary);
      await writeFile(path.join(other, 'ownership.json'), JSON.stringify(owner));
      await assert.rejects(checkEvidence(other, '3'));
    }
    await writeFile(path.join(root, 'ownership.json'), JSON.stringify({ ...owner, shard: '7' }));
    await assert.rejects(checkEvidence(root, '3'));
  } finally { await rm(temporary, { recursive: true, force: true }); }
});

test('workflow enumerates the same eight shards with isolated artifact names and strict wrapper validation', async () => {
  const workflow = await readFile(new URL('../../.github/workflows/windows-browser-control.yml', import.meta.url), 'utf8');
  const ci = await readFile(new URL('../../.github/workflows/ci.yml', import.meta.url), 'utf8');
  const wrapper = await readFile(new URL('../../scripts/windows-browser-control.ps1', import.meta.url), 'utf8');
  const child = await readFile(new URL('../../scripts/windows-browser-control-child.mjs', import.meta.url), 'utf8');
  assert.match(workflow, /fail-fast: false/);
  const matrix = 'shard: [1, 2, 3, 4, 5, 6, 7, 8]';
  assert.ok(workflow.includes(matrix)); assert.ok(ci.includes(matrix));
  assert.ok(workflow.includes('THEME_SHARD: ${{ matrix.shard }}'));
  const names = workflow.split('\n').filter(line => /^          name: browser-control/.test(line));
  assert.equal(names.length, 3);
  for (const name of names) assert.ok(name.includes('${{ matrix.shard }}'));
  for (const line of workflow.split('\n').filter(line => line.includes('test-results/windows-'))) {
    assert.ok(line.includes('-${{ matrix.shard }}/'));
  }
  assert.doesNotMatch(workflow, /continue-on-error|windows-themes-7|windows-browser-control-7/);
  assert.ok(wrapper.includes("$env:THEME_SHARD -cnotmatch '\\A[1-8]\\z'"));
  assert.ok(wrapper.includes('schema = 2; shard = $shard;'));
  assert.ok(wrapper.includes("$controlDirectory = 'windows-browser-control-' + $shard"));
  assert.ok(child.includes('const diagnostic = diagnosticPlan(process.env)'));
  assert.ok(child.includes('diagnostic.output'));
});
