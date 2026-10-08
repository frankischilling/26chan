// Owned Windows qualification only. No browser feature, test argument, retry,
// deadline or pass/fail rule is changed. No raw debug stream is persisted.
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { lstat, mkdir, realpath, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { constants } from 'node:os';
import { performance } from 'node:perf_hooks';
import filter from './windows-theme-stderr-filter.cjs';

const { createStderrCollector, ARTIFACT_BYTES } = filter;
const root = fileURLToPath(new URL('../', import.meta.url));
const SIGNALS = new Set(['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGKILL', 'SIGBREAK', 'SIGABRT', 'SIGSEGV', 'SIGILL', 'SIGBUS', 'SIGFPE']);

export function themeLaunchPlan(argv, env, platform = process.platform) {
  const shard = env.THEME_SHARD;
  const expected = ['--shard', `${shard}/8`, '--output', `test-results/windows-themes-${shard}`];
  if (platform !== 'win32' || !/^[1-8]$/.test(shard ?? '') ||
      env.WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS !== '1' || env.WINDOWS_VISUAL_NETLOG !== '1' ||
      argv.length !== expected.length || argv.some((arg, index) => arg !== expected[index])) {
    throw new Error('Owned Windows theme launcher configuration rejected');
  }
  const childEnv = { ...env, DEBUG: 'pw:browser', DEBUG_COLORS: '0', WINDOWS_THEME_STDERR_PROBE: '1' };
  // Never allow the pinned debug sink to be replaced by a raw file logger.
  delete childEnv.DEBUG_FILE;
  return {
    args: ['test', '--config', 'playwright.themes.config.js', ...argv],
    env: childEnv,
    output: expected[3],
  };
}

export function normalizeChildOutcome(code, signal, failedToSpawn = false) {
  if (failedToSpawn) return { kind: 'spawn-error', code: 1, signal: null };
  if (typeof signal === 'string' && SIGNALS.has(signal)) {
    return { kind: 'signal', code: 128 + (constants.signals[signal] ?? 1), signal };
  }
  if (signal === null && Number.isInteger(code) && code >= -2147483648 && code <= 4294967295) {
    return { kind: 'exit', code, signal: null };
  }
  return { kind: 'unavailable', code: 1, signal: null };
}

export async function runThemeChild({ command, args, env, cwd }, {
  spawnChild = spawn, collector = createStderrCollector({ now: () => performance.now() }), signals = process,
} = {}) {
  let child;
  try {
    child = spawnChild(command, args, { cwd, env, shell: false, detached: false, stdio: ['inherit', 'inherit', 'pipe'] });
  } catch {
    collector.finish();
    return { outcome: normalizeChildOutcome(null, null, true), diagnostic: collector.snapshot(), stream_error: false };
  }
  let failedToSpawn = false;
  let streamError = false;
  const forwards = new Map();
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    const forward = () => { try { child.kill(signal); } catch { streamError = true; } };
    forwards.set(signal, forward);
    signals.on(signal, forward);
  }
  const outcome = await new Promise(resolve => {
    child.once('error', () => { failedToSpawn = true; });
    if (child.stderr) {
      child.stderr.on('data', chunk => collector.push(chunk));
      child.stderr.on('error', () => { streamError = true; });
    } else streamError = true;
    child.once('close', (code, signal) => resolve(normalizeChildOutcome(code, signal, failedToSpawn)));
  });
  for (const [signal, forward] of forwards) signals.off(signal, forward);
  collector.finish();
  const diagnostic = collector.snapshot();
  if (diagnostic.setup_refused) outcome.kind = 'diagnostic-setup-refused';
  return { outcome, diagnostic, stream_error: streamError };
}

export function encodeEvidence(result) {
  const body = JSON.stringify({
    ...result.diagnostic,
    child: result.outcome,
    stream_error: result.stream_error,
    network_child_stderr_coverage: 'unverified',
    absent_match: 'inconclusive',
  });
  if (Buffer.byteLength(body, 'utf8') > ARTIFACT_BYTES) throw new Error('Owned stderr evidence exceeds byte limit');
  return body;
}

export async function saveEvidence(projectRoot, output, body) {
  if (!/^test-results\/windows-themes-[1-8]$/.test(output) || Buffer.byteLength(body, 'utf8') > ARTIFACT_BYTES) {
    throw new Error('Owned stderr artifact rejected');
  }
  const canonicalRoot = await realpath(projectRoot);
  let directory = canonicalRoot;
  for (const segment of output.split('/')) {
    directory = path.join(directory, segment);
    try { await mkdir(directory); } catch (error) { if (error.code !== 'EEXIST') throw error; }
    const entry = await lstat(directory);
    if (!entry.isDirectory() || entry.isSymbolicLink() || await realpath(directory) !== directory) {
      throw new Error('Owned stderr artifact path rejected');
    }
  }
  // A prior file/link is never replaced; Playwright owns output cleanup.
  await writeFile(path.join(directory, 'transport-stderr.json'), body, { flag: 'wx', mode: 0o600 });
}

async function main() {
  let plan;
  try { plan = themeLaunchPlan(process.argv.slice(2), process.env); }
  catch { console.error('Owned Windows theme launcher configuration rejected.'); process.exitCode = 1; return; }
  let cli;
  try { cli = createRequire(import.meta.url).resolve('@playwright/test/cli'); }
  catch { console.error('Owned Windows theme CLI unavailable.'); process.exitCode = 1; return; }
  const preload = fileURLToPath(new URL('./windows-theme-stderr-preload.cjs', import.meta.url));
  const result = await runThemeChild({ command: process.execPath, args: ['--require', preload, cli, ...plan.args], env: plan.env, cwd: root });
  try {
    await saveEvidence(root, plan.output, encodeEvidence(result));
  } catch { console.error('Synthetic Windows transport stderr evidence unavailable.'); }
  // Diagnostic collection never changes the original exit code. Re-raise a
  // child termination signal where supported, with a nonzero fallback.
  process.exitCode = result.outcome.code;
  if (result.outcome.code !== 0) console.error('Owned Windows theme child failed; its original exit status is preserved.');
  if (result.outcome.signal) {
    try { process.kill(process.pid, result.outcome.signal); } catch { }
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(() => { console.error('Owned Windows theme launcher failed.'); process.exitCode = 1; });
}
