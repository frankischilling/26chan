import { spawn } from 'node:child_process';
import { performance } from 'node:perf_hooks';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { lstat, realpath, writeFile } from 'node:fs/promises';
import { themeLaunchPlan, runThemeChild, encodeEvidence, saveEvidence } from './windows-theme-stderr.mjs';
import filter from './windows-theme-stderr-filter.cjs';
import { diagnosticPlan, createTriggerCollector, bounded, encodeSummary, STARTUP_MS, NAVIGATION_MS, CLOSE_MS } from './windows-browser-control-core.mjs';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const NAVIGATION = new Set(['not-triggered', 'http-200', 'unexpected-response', 'failed-or-timeout', 'unavailable']);
const NETLOG = new Set(['not-triggered', 'accepted', 'discarded-success', 'incomplete-close', 'unavailable', 'unsafe-path', 'size-limit', 'existing-path', 'incomplete', 'file-limit']);

export async function saveControlSummary(root, value) {
  if (await realpath(root) !== root || !(await lstat(root)).isDirectory()) throw new Error('Unsafe control root');
  await writeFile(path.join(root, 'summary.json'), encodeSummary(value), { flag: 'wx', mode: 0o600 });
}

export async function coordinate({ startControl, runOriginal, now = () => performance.now(), startupMs = STARTUP_MS + 2000 }) {
  const origin = now();
  const elapsed = () => {
    const value = now() - origin;
    return Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER ? Math.floor(value) : null;
  };
  const summary = { schema: 1, diagnostic: 'one-shot-browser-control', phase: 'control-startup',
    conclusion: 'inconclusive', trigger: 'not-observed', control: 'unavailable', package_version: null,
    trigger_receipt_ms: null, dispatch_ms: null, started_receipt_ms: null, completion_receipt_ms: null,
    trigger_to_completion_ms: null, clock: 'coordinator-monotonic-receipts',
    original: null, navigation: 'not-triggered', status: null, network_error: null, netlog: 'not-triggered',
    tcp: null, control_closed: false, control_process_exited: false, original_stderr_artifact: 'unavailable' };
  let child, readyResolve, finishResolve, triggered = false, startupOpen = true;
  const ready = new Promise(resolve => { readyResolve = resolve; });
  const finished = new Promise(resolve => { finishResolve = resolve; });
  let exitResolve;
  const exited = new Promise(resolve => { exitResolve = resolve; });
  const send = type => {
    try { if (child?.connected) child.send({ type }, () => {}); } catch { }
  };
  try {
    child = startControl();
    child.on('error', () => { readyResolve(false); finishResolve(false); });
    child.on('exit', () => { readyResolve(false); finishResolve(false); });
    child.on('close', () => exitResolve(true));
    child.on('message', message => {
      if (!message || typeof message !== 'object') return;
      if (message.type === 'ready' && startupOpen && message.package_version === '1.62.0') {
        summary.control = 'ready'; summary.package_version = '1.62.0'; readyResolve(true);
      } else if (message.type === 'unavailable') {
        summary.control = 'unavailable'; readyResolve(false); finishResolve(false);
      } else if (message.type === 'started' && triggered && summary.started_receipt_ms === null) {
        summary.started_receipt_ms = elapsed();
      } else if (message.type === 'completed' && triggered && summary.completion_receipt_ms === null) {
        summary.completion_receipt_ms = elapsed();
        if (NAVIGATION.has(message.navigation)) summary.navigation = message.navigation;
        if (Number.isInteger(message.status) && message.status >= 100 && message.status <= 599) summary.status = message.status;
        if (typeof message.network_error === 'string' && /^net::[A-Z0-9_]{1,80}$/.test(message.network_error)) summary.network_error = message.network_error;
      } else if (message.type === 'finished') {
        if (NETLOG.has(message.netlog)) summary.netlog = message.netlog;
        summary.control_closed = message.closed === true;
        const tcp = message.tcp;
        if (tcp && ['observed', 'unverified'].includes(tcp.successful_tcp) && ['observed', 'unverified'].includes(tcp.fresh_tcp) && Number.isInteger(tcp.attempts) && tcp.attempts >= 0 && tcp.attempts <= 65535 &&
            [tcp.request_time, tcp.connect_time, tcp.connect_end_time].every(time => time === null || typeof time === 'string' && /^[0-9.]{1,32}$/.test(time))) {
          summary.tcp = { fresh_tcp: tcp.fresh_tcp, successful_tcp: tcp.successful_tcp, attempts: tcp.attempts,
            request_time: tcp.request_time, connect_time: tcp.connect_time, connect_end_time: tcp.connect_end_time, clock_domain: 'control-netlog-only' };
        }
        finishResolve(true);
      }
    });
  } catch { readyResolve(false); finishResolve(false); }
  const available = await bounded(ready, startupMs, false);
  startupOpen = false;
  if (!available) { summary.control = 'unavailable'; send('stop'); }
  const trigger = () => {
    if (triggered) return;
    triggered = true;
    summary.trigger = 'primary-no-buffer-space'; summary.trigger_receipt_ms = elapsed();
    if (!available || summary.control !== 'ready') return;
    summary.dispatch_ms = elapsed(); send('navigate');
  };
  summary.phase = 'original-running';
  try {
    const original = await runOriginal(trigger);
    summary.original = original.outcome;
    summary.original_stderr_artifact = original.evidenceSaved ? 'saved' : 'unavailable';
  } finally {
    summary.phase = 'control-closing'; send('stop');
    await bounded(finished, NAVIGATION_MS + CLOSE_MS + 2000, false);
    summary.control_process_exited = await bounded(exited, CLOSE_MS, false);
    if (child?.connected) { try { child.disconnect(); } catch { } }
    child?.unref();
  }
  if (summary.trigger_receipt_ms !== null && summary.completion_receipt_ms !== null) {
    summary.trigger_to_completion_ms = summary.completion_receipt_ms - summary.trigger_receipt_ms;
  }
  // Positive evidence only; neither outcome identifies the original cause.
  if (summary.navigation === 'http-200' && summary.tcp?.successful_tcp === 'observed' && summary.netlog === 'accepted') {
    summary.conclusion = 'control-connected-after-observed-failure';
  }
  summary.phase = 'finished';
  return summary;
}

async function main() {
  const diagnostic = diagnosticPlan(process.env);
  const root = path.resolve(ROOT, diagnostic.output);
  const plan = themeLaunchPlan(diagnostic.args, process.env);
  const cli = createRequire(import.meta.url).resolve('@playwright/test/cli');
  const preload = fileURLToPath(new URL('./windows-theme-stderr-preload.cjs', import.meta.url));
  const summary = await coordinate({
    startControl: () => {
      const env = { ...process.env, DEBUG: '', WINDOWS_THEME_STDERR_PROBE: '0' };
      delete env.DEBUG_FILE;
      return spawn(process.execPath, [fileURLToPath(new URL('./windows-browser-control-child.mjs', import.meta.url))],
        { cwd: ROOT, env, shell: false, detached: false, stdio: ['ignore', 'ignore', 'ignore', 'ipc'] });
    },
    runOriginal: async trigger => {
      const diagnosticCollector = filter.createStderrCollector({ now: () => performance.now() });
      const consumeTrigger = createTriggerCollector(trigger);
      const collector = { push: chunk => { diagnosticCollector.push(chunk); consumeTrigger(chunk); },
        finish: () => diagnosticCollector.finish(), snapshot: () => diagnosticCollector.snapshot() };
      const result = await runThemeChild({ command: process.execPath, args: ['--require', preload, cli, ...plan.args], env: plan.env, cwd: ROOT }, { collector });
      let evidenceSaved = false;
      try { await saveEvidence(ROOT, plan.output, encodeEvidence(result)); evidenceSaved = true; } catch { }
      return { ...result, evidenceSaved };
    },
  });
  const code = summary.original?.code ?? 1;
  try { await saveControlSummary(root, summary); } catch { }
  // The containing Job Object handles any child that missed its close deadline.
  // There are no remaining original-runner streams when runThemeChild resolves.
  process.exit(code);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(() => { process.exit(1); });
}
